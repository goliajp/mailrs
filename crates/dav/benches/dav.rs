//! Microbenchmarks for the pure helpers + sync composition paths (no live
//! store hits).
//!
//! For async handler benches that DO hit an in-memory store, see
//! `benches/handlers.rs`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use mailrs_dav::parse::{extract_ical_datetime, extract_ical_field, extract_multiget_uids};
use mailrs_dav::principal::principal_propfind;
use mailrs_dav::xml::{etag_of, multistatus, xml_escape};

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

fn bench_principal(c: &mut Criterion) {
    // Empty request body — "give me everything" form most clients send first.
    c.bench_function("principal_propfind_empty_body", |b| {
        b.iter(|| principal_propfind(black_box("alice@example.com"), black_box("")))
    });

    // Specific prop request — narrow selection (current-user-principal only).
    let narrow_req = "<D:propfind><D:prop><D:current-user-principal/></D:prop></D:propfind>";
    c.bench_function("principal_propfind_narrow_props", |b| {
        b.iter(|| principal_propfind(black_box("alice@example.com"), black_box(narrow_req)))
    });
}

fn bench_multistatus_sizes(c: &mut Criterion) {
    // Small inner body — typical single-resource PROPFIND Depth=0 response.
    let small = "<D:response>\
                 <D:href>/dav/calendars/u/Work/</D:href>\
                 <D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop>\
                 <D:status>HTTP/1.1 200 OK</D:status></D:propstat>\
                 </D:response>";
    c.bench_function("multistatus_wrap_small", |b| {
        b.iter(|| multistatus(black_box(small)))
    });

    // Medium inner body — ~20 events listed in a calendar PROPFIND Depth=1.
    let med = (0..20)
        .map(|i| {
            format!(
                "<D:response><D:href>/dav/calendars/u/Work/evt-{i}.ics</D:href>\
                 <D:propstat><D:prop><D:getetag>\"abc{i:016x}\"</D:getetag>\
                 <D:getcontenttype>text/calendar; charset=utf-8</D:getcontenttype></D:prop>\
                 <D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>"
            )
        })
        .collect::<String>();
    c.bench_function("multistatus_wrap_med_20", |b| {
        b.iter(|| multistatus(black_box(&med)))
    });

    // Large inner body — ~200 events, REPORT multiget with calendar-data.
    let large = (0..200)
        .map(|i| {
            format!(
                "<D:response><D:href>/dav/calendars/u/Work/evt-{i}.ics</D:href>\
                 <D:propstat><D:prop><D:getetag>\"abc{i:016x}\"</D:getetag>\
                 <C:calendar-data>BEGIN:VEVENT\nUID:evt-{i}\nSUMMARY:event {i}\nEND:VEVENT</C:calendar-data></D:prop>\
                 <D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>"
            )
        })
        .collect::<String>();
    c.bench_function("multistatus_wrap_large_200", |b| {
        b.iter(|| multistatus(black_box(&large)))
    });
}

fn bench_etag_sizes(c: &mut Criterion) {
    // SHA-256 on a small payload (vCard / single VEVENT body).
    let small = "BEGIN:VCARD\nUID:abc\nFN:John Doe\nEMAIL:john@example.com\nEND:VCARD";
    c.bench_function("etag_of_small_60b", |b| {
        b.iter(|| etag_of(black_box(small)))
    });

    // 4 KB payload — realistic VEVENT with description / attendees / VTIMEZONE.
    let med = "BEGIN:VEVENT\n".to_string()
        + &"X-CUSTOM:".repeat(400)
        + "\nEND:VEVENT";
    c.bench_function("etag_of_med_4kb", |b| {
        b.iter(|| etag_of(black_box(&med)))
    });
}

criterion_group!(
    benches,
    bench_xml,
    bench_parse,
    bench_multiget,
    bench_principal,
    bench_multistatus_sizes,
    bench_etag_sizes
);
criterion_main!(benches);
