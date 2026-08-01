# prod-only line count for a Rust source: everything except a trailing
# `#[cfg(test)] mod tests { ... }` block. Community convention keeps unit
# tests inline next to the code they test, and counting them would
# penalise a file for being well tested.
BEGIN { in_test = 0; depth = 0; n = 0 }
in_test == 0 {
    if ($0 ~ /^[[:space:]]*#\[cfg\(test\)\][[:space:]]*$/) {
        if ((getline next_line) > 0) {
            if (next_line ~ /^[[:space:]]*mod[[:space:]]+tests[[:space:]]*\{[[:space:]]*$/) {
                in_test = 1; depth = 1; next
            }
            n += 2; next
        }
        n += 1; next
    }
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
END { print n }
