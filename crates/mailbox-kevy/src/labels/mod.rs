//! Labels, per user: the definitions in one hash, and for each label the set
//! of conversations it is on. Applying a label is the user's own act, a
//! fact; nothing here is derived. A list by label reads the set and sorts
//! the rows it names, so no ordering is kept by hand.

use std::collections::HashMap;
use std::io;

use mailrs_mailbox::labels::{Label, LabelError, label_color, label_name, name_taken};

use crate::KevyMailboxStore;
use crate::keys;

/// A label operation refused, or the store failing.
#[derive(Debug, thiserror::Error)]
pub enum LabelOpError {
    #[error(transparent)]
    Label(#[from] LabelError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl KevyMailboxStore {
    /// `user`'s labels, by name.
    pub fn labels(&self, user: &str) -> io::Result<Vec<Label>> {
        let raw = self.store().hgetall(keys::user_labels(user).as_bytes())?;
        let mut out: Vec<Label> = raw
            .iter()
            .filter_map(|(_, v)| serde_json::from_slice(v).ok())
            .collect();
        out.sort_by_key(|l| l.name.to_lowercase());
        Ok(out)
    }

    fn label(&self, user: &str, id: &str) -> io::Result<Option<Label>> {
        let raw = self
            .store()
            .hget(keys::user_labels(user).as_bytes(), id.as_bytes())?;
        Ok(raw.and_then(|v| serde_json::from_slice(&v).ok()))
    }

    fn put_label(&self, user: &str, label: &Label) -> io::Result<()> {
        let json = serde_json::to_vec(label).map_err(io::Error::other)?;
        self.store().hset(
            keys::user_labels(user).as_bytes(),
            &[(label.id.as_bytes(), json.as_slice())],
        )?;
        Ok(())
    }

    /// A new label. Ids count up per user from 1.
    pub fn create_label(
        &self,
        user: &str,
        name: &str,
        color: &str,
        now: i64,
    ) -> Result<Label, LabelOpError> {
        let name = label_name(name)?;
        let color = label_color(color)?;
        if name_taken(&self.labels(user)?, &name, None) {
            return Err(LabelError::Taken(name).into());
        }
        let id = self
            .store()
            .incr(keys::user_label_next(user).as_bytes())
            .map_err(io::Error::from)?;
        let label = Label {
            id: id.to_string(),
            name,
            color,
            created_at: now,
        };
        self.put_label(user, &label)?;
        Ok(label)
    }

    /// Rename or recolour; the id and what it is on stay.
    pub fn update_label(
        &self,
        user: &str,
        id: &str,
        name: Option<&str>,
        color: Option<&str>,
    ) -> Result<Label, LabelOpError> {
        let mut label = self.label(user, id)?.ok_or(LabelError::NotFound)?;
        if let Some(n) = name {
            let n = label_name(n)?;
            if name_taken(&self.labels(user)?, &n, Some(id)) {
                return Err(LabelError::Taken(n).into());
            }
            label.name = n;
        }
        if let Some(c) = color {
            label.color = label_color(c)?;
        }
        self.put_label(user, &label)?;
        Ok(label)
    }

    /// The label and its applications; the mail stays. `false` when there
    /// was no such label.
    pub fn delete_label(&self, user: &str, id: &str) -> io::Result<bool> {
        let gone = self
            .store()
            .hdel(keys::user_labels(user).as_bytes(), &[id.as_bytes()])?;
        self.store()
            .del(&[keys::user_label_threads(user, id).as_bytes()])?;
        Ok(gone > 0)
    }

    /// Put the label `id` on `thread_id`, or take it off. Only somebody in
    /// the conversation labels it.
    pub fn set_label(
        &self,
        user: &str,
        id: &str,
        thread_id: &str,
        on: bool,
    ) -> Result<(), LabelOpError> {
        if self.label(user, id)?.is_none() {
            return Err(LabelError::NotFound.into());
        }
        if !self.is_thread_participant(user, thread_id)? {
            return Err(LabelError::NotFound.into());
        }
        let key = keys::user_label_threads(user, id);
        match on {
            true => self.store().sadd(key.as_bytes(), &[thread_id.as_bytes()]),
            false => self.store().srem(key.as_bytes(), &[thread_id.as_bytes()]),
        }
        .map_err(io::Error::from)?;
        Ok(())
    }

    /// The conversations `id` is on, unordered.
    pub fn threads_with_label(&self, user: &str, id: &str) -> io::Result<Vec<String>> {
        let members = self
            .store()
            .smembers(keys::user_label_threads(user, id).as_bytes())?;
        Ok(members
            .into_iter()
            .filter_map(|m| String::from_utf8(m).ok())
            .collect())
    }

    /// Every labelled conversation of `user` and its label ids, for a page
    /// of rows to show their labels from one read per label.
    pub fn labels_by_thread(&self, user: &str) -> io::Result<HashMap<String, Vec<String>>> {
        let mut out: HashMap<String, Vec<String>> = HashMap::new();
        for label in self.labels(user)? {
            for tid in self.threads_with_label(user, &label.id)? {
                out.entry(tid).or_default().push(label.id.clone());
            }
        }
        Ok(out)
    }

    /// A conversation deleted for good leaves every label.
    pub(crate) fn drop_thread_labels(&self, user: &str, thread_id: &str) -> io::Result<()> {
        for label in self.labels(user)? {
            self.store().srem(
                keys::user_label_threads(user, &label.id).as_bytes(),
                &[thread_id.as_bytes()],
            )?;
        }
        Ok(())
    }

    /// A merged conversation keeps the labels of both halves.
    pub(crate) fn move_thread_labels(&self, user: &str, from: &str, into: &str) -> io::Result<()> {
        for label in self.labels(user)? {
            let key = keys::user_label_threads(user, &label.id);
            if self.store().srem(key.as_bytes(), &[from.as_bytes()])? > 0 {
                self.store().sadd(key.as_bytes(), &[into.as_bytes()])?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
