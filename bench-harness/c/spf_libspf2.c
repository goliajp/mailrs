// SPF record parse via libspf2 (https://www.libspf2.org/).
//
// Builds against libspf2 if installed (Homebrew: `brew install libspf2`,
// Debian: `apt install libspf2-dev`). Read corpus from argv[1], parse N
// times, print ns/op.
//
// Compile:
//   cc -O2 spf_libspf2.c -lspf2 -o spf_libspf2
// Run:
//   ./spf_libspf2 ../corpus/spf_simple.txt 1000000

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#ifdef __has_include
#  if __has_include(<spf2/spf.h>)
#    include <spf2/spf.h>
#    define HAVE_LIBSPF2 1
#  endif
#endif

#ifndef HAVE_LIBSPF2
int main(int argc, char **argv) {
    (void)argc; (void)argv;
    fprintf(stderr, "libspf2 headers not found — install libspf2-dev and recompile\n");
    return 2;
}
#else

static char *slurp(const char *path) {
    FILE *f = fopen(path, "r");
    if (!f) { perror(path); exit(1); }
    fseek(f, 0, SEEK_END);
    long n = ftell(f);
    fseek(f, 0, SEEK_SET);
    char *buf = malloc((size_t)n + 1);
    if (!buf) { perror("malloc"); exit(1); }
    fread(buf, 1, (size_t)n, f);
    buf[n] = '\0';
    // strip trailing newline
    if (n > 0 && buf[n-1] == '\n') buf[n-1] = '\0';
    fclose(f);
    return buf;
}

int main(int argc, char **argv) {
    if (argc != 3) {
        fprintf(stderr, "usage: %s <corpus.txt> <iterations>\n", argv[0]);
        return 1;
    }
    char *record = slurp(argv[1]);
    long iters = atol(argv[2]);

    SPF_record_t *r = NULL;
    SPF_server_t *srv = SPF_server_new(SPF_DNS_RESOLV, 0);
    if (!srv) { fprintf(stderr, "SPF_server_new failed\n"); return 1; }

    struct timespec t0, t1;
    clock_gettime(CLOCK_MONOTONIC, &t0);

    for (long i = 0; i < iters; i++) {
        SPF_record_t *parsed = NULL;
        SPF_response_t *resp = NULL;
        SPF_record_compile(srv, &resp, &parsed, record);
        if (parsed) SPF_record_free(parsed);
        if (resp) SPF_response_free(resp);
    }

    clock_gettime(CLOCK_MONOTONIC, &t1);
    SPF_server_free(srv);
    free(record);
    (void)r;

    double elapsed_ns =
        (double)(t1.tv_sec - t0.tv_sec) * 1e9 + (double)(t1.tv_nsec - t0.tv_nsec);
    double ns_per_op = elapsed_ns / (double)iters;
    printf("c/libspf2/parse: %.1f ns/op (%ld iters)\n", ns_per_op, iters);
    return 0;
}
#endif
