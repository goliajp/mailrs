#![deny(missing_docs)]
//! Fraud predicates executed by goliajp/luna. Rust owns facts and actions;
//! Lua owns rule conditions, scores, grades and brand data.

mod facts;
mod reload;
pub mod stored;
pub use reload::{Scan, active_rule_set, scan};

use luna_jit::runtime::Value;
use luna_jit::{Lua, LuaRoot, version::LuaVersion};
use mailrs_fraud::{Facts, Finding, Findings, Layer, Policy};
use sha2::{Digest, Sha256};

/// The complete, independently deployable default rule bundle.
pub const DEFAULT_SOURCE: &str = concat!(
    include_str!("../rules/brands.lua"),
    "\n",
    include_str!("../rules/helpers.lua"),
    "\n",
    include_str!("../rules/identity.lua"),
    "\n",
    include_str!("../rules/provenance.lua"),
    "\n",
    include_str!("../rules/content.lua"),
    "\n",
);
const BUDGET: i64 = 200_000;
const MEMORY: usize = 16 * 1024 * 1024;
/// Maximum source bytes, checked before compiling external rule bundles.
pub const MAX_SOURCE: usize = 256 * 1024;

const BOOTSTRAP: &str = r#"
local registered, ids = {}, {}
-- The sixth argument is optional: `{ shadow = true }` runs a rule and
-- records its hits without scoring them or holding on them.
local function is_shadow(opts)
    if opts == nil then return false end
    assert(type(opts) == 'table', 'invalid rule options')
    for k, v in pairs(opts) do
        assert(k == 'shadow' and type(v) == 'boolean', 'invalid rule option')
    end
    return opts.shadow == true
end
function rule(id, layer, score, hold, check, opts)
    assert(type(id) == 'string' and #id > 0 and #id <= 100 and not ids[id], 'invalid/duplicate rule id')
    assert(layer == 'identity' or layer == 'provenance' or layer == 'content' or layer == 'transport', 'invalid layer')
    assert(type(score) == 'number' and score > 0 and score <= 100, 'invalid score')
    assert(type(hold) == 'boolean' and type(check) == 'function', 'invalid rule')
    local shadow = is_shadow(opts)
    assert(not (shadow and hold), 'a shadow rule cannot hold')
    assert(#registered < 64, 'too many rules')
    ids[id] = true
    registered[#registered+1] = {id, layer, score, hold, check, shadow}
end
function __count() return #registered end
function __shadow_rules()
    local out = {}
    for _, r in ipairs(registered) do
        if r[6] then out[#out+1] = r[1] end
    end
    return table.concat(out, '\n')
end
function __enforced_rules()
    local out = {}
    for _, r in ipairs(registered) do
        if not r[6] then out[#out+1] = r[1] end
    end
    return table.concat(out, '\n')
end
function __run(i)
    local r = registered[i]
    local out = r[5](m)
    if out == nil or out == false then return end
    local detail, params = out, nil
    if type(out) == 'table' then detail, params = out.detail, out.params end
    assert(type(detail) == 'string' and #detail > 0 and #detail <= 2048, 'invalid finding detail')
    local flat, n = {}, 0
    if params ~= nil then
        assert(type(params) == 'table', 'invalid finding params')
        for k, v in pairs(params) do
            n = n + 1
            assert(n <= 8, 'too many finding params')
            assert(type(k) == 'string' and #k > 0 and #k <= 32 and string.find(k, '^[a-z_]+$'), 'invalid finding param name')
            assert(type(v) == 'string' and #v <= 256, 'invalid finding param value')
            flat[#flat+1] = k
            flat[#flat+1] = v
        end
    end
    return r[1], r[2], r[3], r[4], r[6], detail, table.unpack(flat)
end
-- No dynamic code, protected calls (which could catch a spent budget), or I/O.
load, loadfile, dofile, require, pcall, xpcall, collectgarbage, print = nil, nil, nil, nil, nil, nil, nil, nil
getmetatable, setmetatable = nil, nil
"#;

/// One compiled rule set, owned by one OS thread (Luna VMs are not Send).
pub struct Rules {
    lua: Lua,
    run: LuaRoot,
    count: usize,
    version: String,
    shadow: Vec<String>,
    enforced: Vec<String>,
}

/// Findings plus any isolated rule failures. The runtime uses the previous
/// working bundle on a failure, rather than silently treating errors as clean.
pub struct Evaluation {
    /// Successfully evaluated rule findings.
    pub findings: Findings,
    /// Errors, including the index of the failed rule.
    pub errors: Vec<String>,
    /// Hits of shadow rules: recorded for measurement, never scored and
    /// never a reason to hold.
    pub shadow: Findings,
}

impl Rules {
    /// Compile and validate a source bundle in a fresh sandbox.
    pub fn compile(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > MAX_SOURCE {
            return Err("rule bundle must contain 1..=262144 bytes".into());
        }
        let mut lua = Lua::sandbox(LuaVersion::Lua54)
            .open_base()
            .open_string()
            .open_table()
            .open_math()
            .with_instr_budget(BUDGET)
            .with_memory_cap(MEMORY)
            .build();
        lua.vm().set_jit_enabled(false); // Native loops do not tick Luna's budget.
        let fold = lua.create_function(|s: String| mailrs_fraud::impersonation::fold(&s));
        let trim = lua.create_function(|s: String| s.trim().to_string());
        let spaces = lua.create_function(|s: String| {
            s.chars()
                .map(|c| if c.is_whitespace() { ' ' } else { c })
                .collect::<String>()
        });
        let han = lua.create_function(|s: String| {
            s.chars().count() == 1 && s.chars().all(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
        });
        for (name, f) in [
            ("fold", fold),
            ("trim", trim),
            ("spaces", spaces),
            ("is_han", han),
        ] {
            lua.set_global(name, f)
                .map_err(|e| lua.vm().error_text(&e))?;
        }
        lua.eval::<Value>(BOOTSTRAP)
            .map_err(|e| lua.vm().error_text(&e))?;
        lua.vm().set_instr_budget(Some(BUDGET));
        lua.eval::<Value>(source)
            .map_err(|e| lua.vm().error_text(&e))?;
        lua.vm().set_instr_budget(Some(BUDGET));
        let count = lua
            .eval::<i64>("return __count()")
            .map_err(|e| lua.vm().error_text(&e))?;
        if !(1..=64).contains(&count) {
            return Err("rule bundle must register 1..=64 rules".into());
        }
        let shadow = lua
            .eval::<String>("return __shadow_rules()")
            .map_err(|e| lua.vm().error_text(&e))?
            .lines()
            .map(str::to_string)
            .collect();
        let enforced = lua
            .eval::<String>("return __enforced_rules()")
            .map_err(|e| lua.vm().error_text(&e))?
            .lines()
            .map(str::to_string)
            .collect();
        let run = lua
            .eval::<Value>("return __run")
            .map_err(|e| lua.vm().error_text(&e))?;
        lua.unpin_all(); // Globals retain the helpers; only dispatcher needs a host root.
        let run = lua.pin(run);
        Ok(Self {
            lua,
            run,
            count: count as usize,
            version: format!("lua:{:x}", Sha256::digest(source)),
            shadow,
            enforced,
        })
    }

    /// Content-addressed version recorded on verdicts.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Number of registered rules.
    pub fn rule_count(&self) -> usize {
        self.count
    }

    /// The ids of the rules registered as shadow, in registration order.
    pub fn shadow_rules(&self) -> &[String] {
        &self.shadow
    }

    /// The ids of the rules that score or hold (every rule that is not
    /// shadow), in registration order.
    pub fn enforced_rules(&self) -> &[String] {
        &self.enforced
    }

    /// Run every rule with a freshly armed instruction and memory budget.
    pub fn classify(&mut self, facts: &Facts<'_>, policy: &Policy) -> Evaluation {
        let mut result = Evaluation {
            findings: Findings::new(),
            errors: Vec::new(),
            shadow: Findings::new(),
        };
        if let Err(e) = facts::install(&mut self.lua, facts, policy) {
            result.errors.push(e);
            return result;
        }
        for i in 1..=self.count {
            self.lua.vm().set_instr_budget(Some(BUDGET));
            self.lua.vm().set_memory_cap(Some(MEMORY));
            let run = self.run.get(&self.lua);
            let output = self.lua.vm().call_value(run, &[Value::Int(i as i64)]);
            match output {
                Ok(v) if v.is_empty() => {}
                Ok(v) => match finding(&v) {
                    Ok((f, true)) => result.shadow.push(f),
                    Ok((f, false)) => result.findings.push(f),
                    Err(e) => result.errors.push(format!("rule {i}: {e}")),
                },
                Err(e) => result
                    .errors
                    .push(format!("rule {i}: {}", self.lua.vm().error_text(&e))),
            }
        }
        // Drop per-message facts, then collect: no append-only host root pool.
        let _ = self.lua.set_global("m", Value::Nil);
        self.lua.vm().collect_garbage();
        result
    }
}

/// One finding, and whether the rule that made it is a shadow rule.
fn finding(v: &[Value]) -> Result<(Finding, bool), String> {
    let (
        [
            Value::Str(id),
            Value::Str(layer),
            score,
            Value::Bool(hold),
            Value::Bool(shadow),
            Value::Str(detail),
        ],
        params,
    ) = v.split_at(v.len().min(6))
    else {
        return Err("invalid finding shape".into());
    };
    let text = |s: &[u8]| String::from_utf8_lossy(s).into_owned();
    let layer = Layer::parse(&text(layer.as_bytes())).ok_or("invalid layer")?;
    let score = match score {
        Value::Int(n) => *n as f64,
        Value::Float(n) => *n,
        _ => return Err("invalid score".into()),
    };
    if !score.is_finite() || score <= 0.0 || score > 100.0 {
        return Err("invalid score".into());
    }
    let mut f = Finding::scored(text(id.as_bytes()), layer, score, text(detail.as_bytes()));
    f.holds = *hold;
    for pair in params.chunks(2) {
        let [Value::Str(k), Value::Str(v)] = pair else {
            return Err("invalid finding params".into());
        };
        f.params.insert(text(k.as_bytes()), text(v.as_bytes()));
    }
    Ok((f, *shadow))
}
