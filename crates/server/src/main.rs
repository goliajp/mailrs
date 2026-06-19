// Take over heap allocation for the whole mailrs-server binary.
// On Linux this routes every Box / Vec / String through
// `mailrs_mmalloc::MailrsAllocator` — a layered mmap-backed
// allocator forked from goliajp/torajs/torajs-mmalloc. Each layer's
// free path is munmap-on-drop, so there is no glibc-arena
// high-water retention; freed pages return to the OS directly.
//
// On macOS host (dev only) the same type resolves to a delegating
// stub over `std::alloc::System`, so a developer running cargo
// from a laptop sees zero behaviour change.
//
// See:
//   .claude/notes/rss-leak-attribution-allocator-2026-06-18.md
//   .claude/incidents/INC-2026-06-18-session-eviction-by-mem-watchdog.md
#[global_allocator]
static ALLOC: mailrs_mmalloc::MailrsAllocator = mailrs_mmalloc::MailrsAllocator;

#[tokio::main]
async fn main() {
    mailrs_server::run().await;
}
