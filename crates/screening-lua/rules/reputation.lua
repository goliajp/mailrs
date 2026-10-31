-- Reputation rules over this server's own observations. Every listing
-- they cite names its observation, source and age; lifetimes, caps and
-- thresholds come from `policy`.

local hard = { incident = true, manual = true }

local function listed_rule(spec, lists, sources)
    spec.kind = 'reputation'
    spec.check = function(m)
        local hits = {}
        for _, name in ipairs(lists) do
            for _, l in ipairs(live(m[name], sources)) do hits[#hits + 1] = l end
        end
        if #hits > 0 then return hits[1].subject .. ' is listed', cite(hits) end
    end
    rule(spec)
end

listed_rule({ id = 'ip-incident-manual', action = 'junk', score = 0,
    reason = 'It was sent from an address this server has recorded as a source of attacks.',
    inspired_by = "Spamhaus DROP's model of a short, hand-checked list" },
    { 'ip' }, hard)

listed_rule({ id = 'ip-abuse', action = 'score', score = 3, cap = 'abuse',
    reason = 'It was sent from an address that abused this server recently.',
    inspired_by = "rspamd's local ip maps" },
    { 'ip' }, { abuse = true })

listed_rule({ id = 'ip-login-guard', action = 'score', score = 2, cap = 'login_guard',
    reason = 'It was sent from an address that tried to break into accounts here recently.',
    inspired_by = "fail2ban and blocklist.de's practice of short-lived entries" },
    { 'ip' }, { login_guard = true })

listed_rule({ id = 'sender-incident-manual', action = 'junk', score = 0,
    reason = "Its sender's domain is one this server has recorded as sending fraud.",
    inspired_by = "Spamhaus DBL's model of listing domains rather than addresses" },
    { 'envelope', 'header' }, hard)

listed_rule({ id = 'link-incident-manual', action = 'junk', score = 0,
    reason = 'It links to a site this server has recorded as a phishing or malware site.',
    inspired_by = 'URL blocklists (SURBL, URLhaus) matching links rather than senders' },
    { 'links' }, hard)

rule({ id = 'sender-user-junk', kind = 'reputation', action = 'score', score = 2.5, cap = 'user_junk',
    reason = "Several people here moved mail from this sender's domain to Junk recently.",
    inspired_by = 'collaborative filtering (Razor, Pyzor) in SpamAssassin',
    check = function(m) return crosses(m.sender_marks, 'user_junk') end })

rule({ id = 'link-user-junk', kind = 'reputation', action = 'score', score = 2.5, cap = 'user_junk',
    reason = 'Several people here moved mail linking to this site to Junk recently.',
    inspired_by = 'collaborative filtering (Razor, Pyzor) in SpamAssassin',
    check = function(m)
        for _, marks in ipairs(m.link_marks) do
            local detail, ev = crosses(marks, 'user_junk')
            if detail then return detail, ev end
        end
    end })

rule({ id = 'sender-complaint', kind = 'reputation', action = 'score', score = 2, cap = 'complaint',
    reason = "Recipients at other providers reported mail from this sender's domain as spam.",
    inspired_by = 'feedback-loop handling at large mailbox providers',
    check = function(m) return crosses(m.sender_marks, 'complaint') end })

-- Any listing still inside its lifetime, from any source.
local every = { incident = true, manual = true, abuse = true, login_guard = true }

rule({ id = 'auth-fail-listed', kind = 'reputation', action = 'score', score = 2,
    reason = 'It failed sender authentication on top of being listed here.',
    inspired_by = "rspamd's composite rules",
    check = function(m)
        if not (m.dmarc_fail or m.spf_fail) then return end
        local hits = {}
        for _, name in ipairs({ 'ip', 'envelope', 'header', 'links' }) do
            for _, l in ipairs(live(m[name], every)) do hits[#hits + 1] = l end
        end
        if #hits > 0 then return 'failed authentication and ' .. hits[1].subject .. ' is listed', cite(hits) end
    end })
