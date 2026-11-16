//! A purged reader's part of the re-judge's records: their release, recall
//! and unheld marks, their conversations in batches, and what rounds
//! gathered of theirs. A round waiting as a pending batch counts one less
//! for each gathered conversation that goes; a batch left with none is
//! dropped, as there is nothing left to decide.

use std::io;

use super::store::RejudgeStore;

const BATCHES: &str = "rejudge:batches";
const CANDIDATES: &str = "rejudge:candidates:";
const MARKS: [&str; 3] = ["rejudge:released:", "rejudge:recalled:", "rejudge:unheld:"];

/// Whether `s` begins with `address` (in any case) and then `sep`.
fn theirs(s: &str, address: &str, sep: char) -> bool {
    s.len() > address.len()
        && s.is_char_boundary(address.len())
        && s[..address.len()].eq_ignore_ascii_case(address)
        && s[address.len()..].starts_with(sep)
}

/// Remove `address`'s records. Returns how many went.
pub fn forget_reader<S: RejudgeStore>(store: &S, address: &str) -> io::Result<u64> {
    let mut n = 0;
    for mark in MARKS {
        for key in store.state_keys(mark)? {
            if theirs(&key[mark.len()..], address, ':') {
                store.state_del(&key)?;
                n += 1;
            }
        }
    }
    for key in store.state_keys("rejudge:batch:")? {
        for member in store.state_smembers(&key)? {
            if theirs(&member, address, '\t') {
                store.state_srem(&key, &member)?;
                n += 1;
            }
        }
    }
    for key in store.state_keys(CANDIDATES)? {
        let fields: Vec<String> = store
            .state_hgetall(&key)?
            .into_iter()
            .map(|(f, _)| f)
            .collect();
        let gone: Vec<&String> = fields.iter().filter(|f| theirs(f, address, '\t')).collect();
        if gone.is_empty() {
            continue;
        }
        for f in &gone {
            store.state_hdel(&key, f)?;
        }
        n += gone.len() as u64;
        lower(
            store,
            &key[CANDIDATES.len()..],
            gone.len() as u64,
            gone.len() == fields.len(),
        )?;
    }
    Ok(n)
}

/// Take `removed` off pending batch `obs`, dropping it at none, or when
/// `none_left` of its gathered conversations remain.
fn lower<S: RejudgeStore>(store: &S, obs: &str, removed: u64, none_left: bool) -> io::Result<()> {
    let Some(raw) = store.state_hget(BATCHES, obs)? else {
        return Ok(());
    };
    let Ok(mut batch) = serde_json::from_slice::<serde_json::Value>(&raw) else {
        return Ok(());
    };
    if batch.get("pending").and_then(|p| p.as_bool()) != Some(true) {
        return Ok(());
    }
    let would_hold = batch
        .get("would_hold")
        .and_then(|w| w.as_u64())
        .unwrap_or(0);
    let left = would_hold.saturating_sub(removed);
    if left == 0 || none_left {
        return store.state_hdel(BATCHES, obs);
    }
    batch["would_hold"] = left.into();
    let json = serde_json::to_vec(&batch).map_err(io::Error::other)?;
    store.state_hset(BATCHES, obs, &json)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_record_is_its_readers_in_any_spelling_and_no_one_elses() {
        assert!(super::theirs("Bob@x.com\tt1", "bob@x.com", '\t'));
        assert!(super::theirs("bob@x.com:t1", "bob@x.com", ':'));
        assert!(!super::theirs("bob@x.comm\tt1", "bob@x.com", '\t'));
        assert!(!super::theirs("bob@x.com", "bob@x.com", '\t'));
    }
}
