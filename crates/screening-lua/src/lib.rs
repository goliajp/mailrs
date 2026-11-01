#![deny(missing_docs)]
//! Spam and attachment rules executed by goliajp/luna. Rust extracts the
//! facts (`mailrs-reputation`, `mailrs-attachment-scan`); Lua owns every
//! rule, score, lifetime and threshold.

mod facts;
mod reload;
pub mod stored;
pub use reload::{active_version, evaluate};

use luna_jit::runtime::Value;
use luna_jit::{Lua, LuaRoot, version::LuaVersion};
use mailrs_attachment_scan::PartFacts;
use sha2::{Digest, Sha256};

/// The default bundle: the policy data first, then the rules that read it.
pub const DEFAULT_SOURCE: &str = concat!(
    include_str!("../rules/policy.lua"),
    "\n",
    include_str!("../rules/helpers.lua"),
    "\n",
    include_str!("../rules/attachments.lua"),
    "\n",
    include_str!("../rules/reputation.lua"),
    "\n",
);
const BUDGET: i64 = 200_000;
const MEMORY: usize = 16 * 1024 * 1024;
/// Maximum source bytes, checked before compiling an external bundle.
pub const MAX_SOURCE: usize = 256 * 1024;

const BOOTSTRAP: &str = r#"
local registered, ids, spent = {}, {}, {}
function rule(r)
    assert(type(r) == 'table', 'rule takes a table')
    local id = r.id
    assert(type(id) == 'string' and #id > 0 and #id <= 100 and not ids[id], 'invalid/duplicate rule id')
    assert(r.kind == 'attachment' or r.kind == 'reputation', 'invalid kind')
    assert(r.action == 'junk' or r.action == 'score', 'invalid action')
    assert(type(r.score) == 'number' and r.score >= 0 and r.score <= 100, 'invalid score')
    assert(r.action == 'junk' or r.score > 0, 'a score rule needs a score')
    assert(type(r.reason) == 'string' and #r.reason > 0 and #r.reason <= 500, 'invalid reason')
    assert(type(r.inspired_by) == 'string' and #r.inspired_by <= 500, 'invalid inspired_by')
    assert(r.cap == nil or type(r.cap) == 'string', 'invalid cap')
    assert(type(r.check) == 'function', 'invalid check')
    assert(#registered < 64, 'too many rules')
    ids[id] = true
    registered[#registered + 1] = r
end
function __count() return #registered end
function __begin() spent = {} end
function __run(i)
    local r = registered[i]
    local detail, evidence = r.check(m)
    if detail == nil or detail == false then return end
    assert(type(detail) == 'string' and #detail > 0 and #detail <= 2048, 'invalid detail')
    local score = r.score
    if r.cap then
        local cap = policy.caps[r.cap]
        assert(type(cap) == 'number', 'no cap for ' .. r.cap)
        local used = spent[r.cap] or 0
        score = math.min(score, math.max(cap - used, 0))
        spent[r.cap] = used + score
    end
    local ev = ''
    if evidence ~= nil then
        assert(type(evidence) == 'table' and #evidence <= 50, 'invalid evidence')
        for _, e in ipairs(evidence) do assert(type(e) == 'string' and #e <= 400, 'invalid evidence') end
        ev = table.concat(evidence, '\n')
    end
    return r.id, r.kind, r.action, score, r.reason, r.inspired_by, detail, ev
end
-- No dynamic code, protected calls (which could catch a spent budget), or I/O.
load, loadfile, dofile, require, pcall, xpcall, collectgarbage, print = nil, nil, nil, nil, nil, nil, nil, nil
getmetatable, setmetatable = nil, nil
"#;

/// What the rules are run against.
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    /// The sender is not one of this server's own domains.
    pub outside: bool,
    /// What each attachment holds.
    pub parts: &'a [PartFacts],
    /// What this server has observed about the sender and links.
    pub reputation: &'a mailrs_reputation::Facts,
}

/// What a rule does when it matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Put the message in Junk, with the reason shown to the reader.
    Junk,
    /// Add to the message's score.
    Score,
}

/// One rule that matched.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    /// The rule's id.
    pub rule: String,
    /// `attachment` or `reputation`.
    pub kind: String,
    /// What it does.
    pub action: Action,
    /// What it adds, after its source's cap.
    pub score: f64,
    /// The reason shown to the reader.
    pub reason: String,
    /// The project or practice the rule's idea came from.
    pub inspired_by: String,
    /// What it matched in this message.
    pub detail: String,
    /// The observations and parts behind it, one per line.
    pub evidence: Vec<String>,
}

/// Every hit, and the bundle that produced them.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// Hits in rule order.
    pub hits: Vec<Hit>,
    /// `lua:<sha256>` of the bundle, with the rejected one on a fallback.
    pub version: String,
}

impl Verdict {
    /// A rule asks for Junk.
    pub fn junk(&self) -> bool {
        self.hits.iter().any(|h| h.action == Action::Junk)
    }

    /// The sum of the scoring rules.
    pub fn score(&self) -> f64 {
        self.hits.iter().map(|h| h.score).sum()
    }
}

/// One compiled bundle, owned by one OS thread (Luna VMs are not Send).
pub struct Rules {
    lua: Lua,
    begin: LuaRoot,
    run: LuaRoot,
    count: usize,
    version: String,
}

/// Hits plus any isolated rule failures.
pub struct Evaluation {
    /// Hits from the rules that ran.
    pub hits: Vec<Hit>,
    /// Errors, each naming the failed rule's index.
    pub errors: Vec<String>,
}

impl Rules {
    /// Compile and validate a bundle in a fresh sandbox.
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
        lua.vm().set_jit_enabled(false); // native loops do not tick the budget
        let text = |lua: &mut Lua, e| lua.vm().error_text(&e);
        lua.eval::<Value>(BOOTSTRAP)
            .map_err(|e| text(&mut lua, e))?;
        lua.vm().set_instr_budget(Some(BUDGET));
        lua.eval::<Value>(source).map_err(|e| text(&mut lua, e))?;
        lua.vm().set_instr_budget(Some(BUDGET));
        let ok = lua
            .eval::<bool>("return type(policy) == 'table' and type(policy.caps) == 'table' and type(policy.lifetime_days) == 'table'")
            .map_err(|e| text(&mut lua, e))?;
        if !ok {
            return Err("rule bundle must define policy with caps and lifetime_days".into());
        }
        let count = lua
            .eval::<i64>("return __count()")
            .map_err(|e| text(&mut lua, e))?;
        if !(1..=64).contains(&count) {
            return Err("rule bundle must register 1..=64 rules".into());
        }
        let begin = lua
            .eval::<Value>("return __begin")
            .map_err(|e| text(&mut lua, e))?;
        let run = lua
            .eval::<Value>("return __run")
            .map_err(|e| text(&mut lua, e))?;
        lua.unpin_all(); // globals keep the rules; only the two entry points need a host root
        let begin = lua.pin(begin);
        let run = lua.pin(run);
        Ok(Self {
            lua,
            begin,
            run,
            count: count as usize,
            version: version_of(source),
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
    pub fn evaluate(&mut self, input: &Input<'_>) -> Evaluation {
        let mut result = Evaluation {
            hits: Vec::new(),
            errors: Vec::new(),
        };
        if let Err(e) = facts::install(&mut self.lua, input) {
            result.errors.push(e);
            return result;
        }
        let begin = self.begin.get(&self.lua);
        if let Err(e) = self.lua.vm().call_value(begin, &[]) {
            result.errors.push(self.lua.vm().error_text(&e));
            return result;
        }
        for i in 1..=self.count {
            self.lua.vm().set_instr_budget(Some(BUDGET));
            self.lua.vm().set_memory_cap(Some(MEMORY));
            let run = self.run.get(&self.lua);
            match self.lua.vm().call_value(run, &[Value::Int(i as i64)]) {
                Ok(v) if v.is_empty() => {}
                Ok(v) => match hit(&v) {
                    Ok(h) => result.hits.push(h),
                    Err(e) => result.errors.push(format!("rule {i}: {e}")),
                },
                Err(e) => result
                    .errors
                    .push(format!("rule {i}: {}", self.lua.vm().error_text(&e))),
            }
        }
        // drop the per-message facts, then collect: no append-only host root pool
        let _ = self.lua.set_global("m", Value::Nil);
        self.lua.vm().collect_garbage();
        result
    }
}

/// The approved bundle versions, with the replay report each came from.
pub const APPROVED: &str = include_str!("../rules/APPROVED");

/// The bundle a verdict ran on is approved. After a fallback, that is the
/// bundle fallen back to.
pub fn approved(version: &str) -> bool {
    let running = version.split(';').next().unwrap_or("");
    APPROVED
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .any(|l| l.split_whitespace().next() == Some(running))
}

/// `lua:<sha256>` of a bundle's source.
pub fn version_of(source: &str) -> String {
    format!("lua:{:x}", Sha256::digest(source))
}

fn hit(v: &[Value]) -> Result<Hit, String> {
    let [
        Value::Str(id),
        Value::Str(kind),
        Value::Str(action),
        score,
        Value::Str(reason),
        Value::Str(inspired_by),
        Value::Str(detail),
        Value::Str(evidence),
    ] = v
    else {
        return Err("invalid hit shape".into());
    };
    let text = |s: &[u8]| String::from_utf8_lossy(s).into_owned();
    let action = match action.as_bytes() {
        b"junk" => Action::Junk,
        b"score" => Action::Score,
        _ => return Err("invalid action".into()),
    };
    let score = match score {
        Value::Int(n) => *n as f64,
        Value::Float(n) => *n,
        _ => return Err("invalid score".into()),
    };
    if !score.is_finite() || !(0.0..=100.0).contains(&score) {
        return Err("invalid score".into());
    }
    let evidence = text(evidence.as_bytes());
    Ok(Hit {
        rule: text(id.as_bytes()),
        kind: text(kind.as_bytes()),
        action,
        score,
        reason: text(reason.as_bytes()),
        inspired_by: text(inspired_by.as_bytes()),
        detail: text(detail.as_bytes()),
        evidence: evidence.lines().map(str::to_string).collect(),
    })
}
