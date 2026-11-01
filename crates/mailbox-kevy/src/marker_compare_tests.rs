//! The read comparison finds nothing on agreeing indexes and finds a row
//! the marker index lost.

use crate::keys;
use crate::marker_tests::{store, varied};

#[test]
fn agreeing_indexes_compare_equal_and_the_comparison_saw_rows() {
    let st = store();
    varied(&st);
    let r = st.threaduser_read_compare().unwrap();
    assert_eq!(r.users, 2);
    // two users, seven flags, three predicate stacks, list and count; three timestamps
    assert_eq!(r.comparisons, 2 * 7 * 3 * 2 + 3);
    assert!(r.rows_returned > 0, "{r:?}");
    assert_eq!(r.differences, 0, "{r:?}");
}

#[test]
fn a_marker_missing_from_a_row_is_a_difference() {
    let st = store();
    varied(&st);
    // starred t0, written behind the writers' back
    let key = keys::thread_user("me@x.test", "t0");
    st.store()
        .hdel(key.as_bytes(), &[b"ix_starred".as_slice()])
        .unwrap();
    let r = st.threaduser_read_compare().unwrap();
    assert!(r.differences > 0, "{r:?}");
    assert!(
        r.samples.iter().any(|s| s.starts_with("me@x.test starred")),
        "{r:?}"
    );
}
