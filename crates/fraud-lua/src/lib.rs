#![deny(missing_docs)]
//! Fraud predicates executed by goliajp/luna. Rust owns facts and actions;
//! Lua owns rule conditions, scores, grades and brand data.

mod facts;
mod reload;
pub use reload::scan;

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
function rule(id, layer, score, hold, check)
    assert(type(id) == 'string' and #id > 0 and #id <= 100 and not ids[id], 'invalid/duplicate rule id')
    assert(layer == 'identity' or layer == 'provenance' or layer == 'content' or layer == 'transport', 'invalid layer')
    assert(type(score) == 'number' and score > 0 and score <= 100, 'invalid score')
    assert(type(hold) == 'boolean' and type(check) == 'function', 'invalid rule')
    assert(#registered < 64, 'too many rules')
    ids[id] = true
    registered[#registered+1] = {id, layer, score, hold, check}
end
function __count() return #registered end
function __run(i)
    local r = registered[i]
    local detail = r[5](m)
    if detail == nil or detail == false then return end
    assert(type(detail) == 'string' and #detail > 0 and #detail <= 2048, 'invalid finding detail')
    return r[1], r[2], r[3], r[4], detail
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
}

/// Findings plus any isolated rule failures. The runtime uses the previous
/// working bundle on a failure, rather than silently treating errors as clean.
pub struct Evaluation {
    /// Successfully evaluated rule findings.
    pub findings: Findings,
    /// Errors, including the index of the failed rule.
    pub errors: Vec<String>,
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

    /// Run every rule with a freshly armed instruction and memory budget.
    pub fn classify(&mut self, facts: &Facts<'_>, policy: &Policy) -> Evaluation {
        let mut result = Evaluation {
            findings: Findings::new(),
            errors: Vec::new(),
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
                    Ok(f) => result.findings.push(f),
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

fn finding(v: &[Value]) -> Result<Finding, String> {
    let [
        Value::Str(id),
        Value::Str(layer),
        score,
        Value::Bool(hold),
        Value::Str(detail),
    ] = v
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
    Ok(f)
}
