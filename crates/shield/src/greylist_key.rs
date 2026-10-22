//! Where a greylist triplet's first-seen time is stored. The receiver's
//! greylist stage writes it and the server's Postgres backfill warms it,
//! so both use this one spelling.

/// The kevy key for one triplet (see [`crate::greylist::triplet_key`]).
pub fn store_key(triplet: &str) -> String {
    format!("gl:{triplet}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_greylist_key_keeps_its_prod_spelling() {
        assert_eq!(store_key("1.2.3.4|s@x|r@y"), "gl:1.2.3.4|s@x|r@y");
    }
}
