# prod-only line count for Rust sources: everything except a trailing
# `#[cfg(test)] mod tests { ... }` block. Community convention keeps unit
# tests inline next to the code they test, and counting them would
# penalise a file for being well tested.
#
# Takes any number of files and prints `<count> <path>` for each, or
# `GEN <path>` when one of its first five lines carries a generated-code
# marker. One process for the whole tree: a process per file cost the
# gate about twenty seconds, most of it in fork and exec.
#
# The line after `#[cfg(test)]` is judged on the next record rather than
# read ahead with getline, which in a multi-file run would read into the
# next file when the attribute is a file's last line.
function flush() {
    if (file == "") return
    if (pending) n += 1
    if (gen) print "GEN " file
    else print n " " file
}
FNR == 1 {
    flush()
    file = FILENAME; in_test = 0; depth = 0; n = 0; pending = 0; gen = 0
}
FNR <= 5 && tolower($0) ~ /codegen:|auto-generated|@generated|do not edit/ { gen = 1 }
pending {
    pending = 0
    if ($0 ~ /^[[:space:]]*mod[[:space:]]+tests[[:space:]]*\{[[:space:]]*$/) {
        in_test = 1; depth = 1; next
    }
    n += 2; next
}
in_test == 0 {
    if ($0 ~ /^[[:space:]]*#\[cfg\(test\)\][[:space:]]*$/) { pending = 1; next }
    n++
    next
}
in_test == 1 {
    for (i = 1; i <= length($0); i++) {
        c = substr($0, i, 1)
        if (c == "{") depth++
        if (c == "}") depth--
    }
    if (depth == 0) in_test = 0
}
END { flush() }
