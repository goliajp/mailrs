// iCalendar parse via libical (https://github.com/libical/libical).
//
// Compile:
//   cc -O2 ical_libical.c $(pkg-config --cflags --libs libical) -o ical_libical
// Run:
//   ./ical_libical ../corpus/ical_simple.ics 100000

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#ifdef __has_include
#  if __has_include(<libical/ical.h>)
#    include <libical/ical.h>
#    define HAVE_LIBICAL 1
#  endif
#endif

#ifndef HAVE_LIBICAL
int main(int argc, char **argv) {
    (void)argc; (void)argv;
    fprintf(stderr, "libical headers not found — install libical and recompile\n");
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
    fread(buf, 1, (size_t)n, f);
    buf[n] = '\0';
    fclose(f);
    return buf;
}

int main(int argc, char **argv) {
    if (argc != 3) {
        fprintf(stderr, "usage: %s <corpus.ics> <iterations>\n", argv[0]);
        return 1;
    }
    char *text = slurp(argv[1]);
    long iters = atol(argv[2]);

    struct timespec t0, t1;
    clock_gettime(CLOCK_MONOTONIC, &t0);

    for (long i = 0; i < iters; i++) {
        icalcomponent *c = icalparser_parse_string(text);
        if (c) icalcomponent_free(c);
    }

    clock_gettime(CLOCK_MONOTONIC, &t1);
    free(text);

    double elapsed_ns =
        (double)(t1.tv_sec - t0.tv_sec) * 1e9 + (double)(t1.tv_nsec - t0.tv_nsec);
    double ns_per_op = elapsed_ns / (double)iters;
    printf("c/libical/parse: %.1f ns/op (%ld iters)\n", ns_per_op, iters);
    return 0;
}
#endif
