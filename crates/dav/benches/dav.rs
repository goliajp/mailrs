//! Microbenchmarks for the pure helpers (no live store hits).

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use mailrs_dav::parse::{extract_ical_datetime, extract_ical_field, extract_multiget_uids};
use mailrs_dav::xml::{etag_of, xml_escape};

fn bench_xml(c: &mut Criterion) {
    c.bench_function("etag_of", |b| b.iter(|| etag_of(black_box("BEGIN:VEVENT\nEND:VEVENT"))));
    c.bench_function("xml_escape_plain", |b| {
        b.iter(|| xml_escape(black_box("plain text without special chars")))
    });
    c.bench_function("xml_escape_with_entities", |b| {
        b.iter(|| xml_escape(black_box("a<b>c&d\"e'f")))
    });
}

fn bench_parse(c: &mut Criterion) {
    let ical = "BEGIN:VEVENT\n\
                SUMMARY:Team Meeting\n\
                DTSTART;TZID=US/Eastern:20240315T120000\n\
                DTEND;TZID=US/Eastern:20240315T130000\n\
                END:VEVENT";
    c.bench_function("extract_ical_field_summary", |b| {
        b.iter(|| extract_ical_field(black_box(ical), black_box("SUMMARY")))
    });
    c.bench_function("extract_ical_datetime_dtstart", |b| {
        b.iter(|| extract_ical_datetime(black_box(ical), black_box("DTSTART")))
    });
}

fn bench_multiget(c: &mut Criterion) {
    let body = "<C:calendar-multiget xmlns:C=\"urn:ietf:params:xml:ns:caldav\">\
                <D:href>/dav/calendars/u/c/abc.ics</D:href>\
                <D:href>/dav/calendars/u/c/def.ics</D:href>\
                <D:href>/dav/calendars/u/c/ghi.ics</D:href>\
                </C:calendar-multiget>";
    c.bench_function("extract_multiget_uids_3", |b| {
        b.iter(|| extract_multiget_uids(black_box(body), black_box(".ics")))
    });
}

criterion_group!(benches, bench_xml, bench_parse, bench_multiget);
criterion_main!(benches);
