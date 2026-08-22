//! What to ask the server for next.
//!
//! The whole of the bookkeeping is one rule: **a uid means nothing
//! outside the uidvalidity it was issued under** (RFC 3501 §2.3.1.1).
//! A client that forgets this carries on from its old highest uid
//! against a renumbered folder, fetches nothing, and reports success —
//! which is the shape of every "my mail stopped arriving and nothing
//! said so" report there is.
//!
//! So the validity travels with the numbers, in one struct, and the
//! plan is `Everything` whenever they disagree.

use crate::parse::Untagged;

/// What a folder looks like right now, and what we remember of it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderState {
    /// Messages the server says are in it.
    pub exists: u32,
    /// The validity the server just announced.
    pub uidvalidity: Option<u32>,
    /// The uid the server will hand out next.
    pub uidnext: Option<u32>,
    /// The validity we stored the last time we synced this folder.
    ///
    /// `None` means we have never opened it, which is not the same as
    /// having opened it and seen a different number.
    pub remembered_uidvalidity: Option<u32>,
}

impl FolderState {
    /// Fold in one untagged response.
    pub fn apply(&mut self, u: &Untagged) {
        match u {
            Untagged::Exists(n) => self.exists = *n,
            Untagged::UidValidity(v) => self.uidvalidity = Some(*v),
            Untagged::UidNext(v) => self.uidnext = Some(*v),
            Untagged::Fetch(_) | Untagged::List(_) => {}
        }
    }
}

/// What to fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchPlan {
    /// Every message in the folder: never synced, or renumbered.
    Everything {
        /// Why, for the log line that explains a sudden full download.
        because: &'static str,
    },
    /// Everything above the uid we already have.
    Since {
        /// The first uid to ask for.
        from: u32,
    },
}

impl FetchPlan {
    /// The `UID FETCH` range this plan asks for.
    pub fn range(&self) -> String {
        match self {
            Self::Everything { .. } => "1:*".to_string(),
            Self::Since { from } => format!("{from}:*"),
        }
    }
}

/// What to ask for, given the folder and the highest uid already held.
///
/// `None` means there is nothing new — which is an answer, not a
/// failure, and the caller should not ask again until something says
/// otherwise.
pub fn plan_fetch(state: &FolderState, highest_held: Option<u32>) -> Option<FetchPlan> {
    let Some(highest) = highest_held else {
        return Some(FetchPlan::Everything {
            because: "this folder has not been synced before",
        });
    };
    match (state.uidvalidity, state.remembered_uidvalidity) {
        // The server renumbered. Every uid we hold means nothing now.
        (Some(now), Some(then)) if now != then => Some(FetchPlan::Everything {
            because: "the server changed UIDVALIDITY, so the uids we held mean nothing",
        }),
        // We have numbers but never recorded which validity they were
        // issued under, so they cannot be trusted either.
        (Some(_), None) => Some(FetchPlan::Everything {
            because: "no UIDVALIDITY was recorded with the uids we hold",
        }),
        _ => match state.uidnext {
            Some(next) if next > highest + 1 => Some(FetchPlan::Since { from: highest + 1 }),
            Some(_) => None,
            // Without UIDNEXT there is nothing to compare, so ask for
            // what is above what we hold and let the server answer.
            None => Some(FetchPlan::Since { from: highest + 1 }),
        },
    }
}
