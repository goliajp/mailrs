-- Data the rules read, kept apart from their code so it can be reviewed on
-- its own. Changing a number here is a rule change: it goes through the
-- same replay as a new rule before it is switched on.
policy = {
    -- How long a deny from each source counts. A source left out never
    -- expires; an administrator retracts it instead.
    lifetime_days = {
        abuse = 90,
        login_guard = 30,
    },
    -- The most one source adds to one message's score, so a source with
    -- many entries cannot decide alone.
    caps = {
        abuse = 4,
        login_guard = 3,
        user_junk = 3,
        complaint = 3,
    },
    -- How many distinct people, and how many marks, inside how many days,
    -- before marks on a domain count.
    user_junk = { min_reporters = 3, min_marks = 5, window_days = 30 },
    complaint = { min_reporters = 2, min_marks = 3, window_days = 30 },
    -- A page that writes a blob out as a file counts as smuggling only with
    -- a base64 run at least this long.
    html_smuggling_min_base64 = 20000,
}
