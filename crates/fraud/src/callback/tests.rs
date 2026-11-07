use super::*;

/// The clients' table: its labels and the samples all three are held to.
fn shared() -> serde_json::Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../web/src/lib/callback-numbers.json"
    );
    let text = std::fs::read_to_string(path).expect("the clients' table");
    serde_json::from_str(&text).expect("json")
}

#[test]
fn the_labels_are_the_clients_labels() {
    let table = shared();
    let labels: Vec<&str> = table["labels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l.as_str().unwrap())
        .collect();
    assert_eq!(labels, LABELS);
}

#[test]
fn every_sample_the_clients_are_held_to_reads_the_same_here() {
    let table = shared();
    let samples = table["samples"].as_array().unwrap();
    assert!(samples.len() >= 10, "the samples were read");
    for s in samples {
        let want: Vec<&str> = s["numbers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().unwrap())
            .collect();
        assert_eq!(
            phone_numbers(s["text"].as_str().unwrap()),
            want,
            "{}",
            s["name"]
        );
    }
}

/// Not in any sample: a number among Arabic text still reads, and
/// Arabic-Indic digits do not (the clients read ASCII digits only).
#[test]
fn a_number_among_other_scripts_reads_and_other_digits_do_not() {
    assert_eq!(
        phone_numbers("للاستفسار اتصل على +971 4 123 4567 شكرا"),
        vec!["+971 4 123 4567"]
    );
    assert!(phone_numbers("اتصل ٠٣-١٢٣٤-٥٦٧٨").is_empty());
}

#[test]
fn a_number_split_across_table_cells_does_not_join() {
    let text = html_text("<table><tr><td>03</td><td>1234-5678</td></tr></table>");
    assert!(phone_numbers(&text).is_empty(), "{text:?}");
    let text = html_text("<p>Call&nbsp;<b>0120-123-456</b> today</p>");
    assert_eq!(phone_numbers(&text), vec!["0120-123-456"]);
    let text = html_text("<style>p{x:1}</style><script>var t='03-1234-5678'</script><p>hi</p>");
    assert!(phone_numbers(&text).is_empty(), "{text:?}");
}

#[test]
fn two_spellings_of_one_number_share_their_digits() {
    assert_eq!(digits("03-1234-5678"), digits("03 1234 5678"));
    assert_eq!(digits("+81 (3) 1234.5678"), "81312345678");
}
