rule('claims-our-name', 'identity', 4.5, true, function(m)
    if not external(m) or m.display == '' then return end
    for _, n in ipairs(m.org_names) do
        if trim(n) ~= '' and contains(m.display, fold(n)) then
            return 'display name claims this organisation'
        end
    end
end)
rule('impersonates-one-of-us', 'identity', 6, true, function(m)
    if not external(m) or m.display == '' then return end
    for _, n in ipairs(m.account_names) do
        if trim(n) ~= '' and m.display == fold(n) then
            return 'the display name is one of this deployment\'s own people, and the domain is not ours'
        end
    end
end)
rule('subject-is-our-name', 'identity', 6, true, function(m)
    if not external(m) or m.subject_folded == '' then return end
    for _, n in ipairs(m.org_names) do
        n = fold(n)
        if n ~= '' then
            local a, b = m.subject_folded:find(n, 1, true)
            if a then
                local rest = m.subject_folded:sub(1, a-1) .. m.subject_folded:sub(b+1)
                if #chars(subject_rest(rest)) <= 4 then
                    return 'the subject is this organisation\'s name and little else, from a domain that is not ours'
                end
            end
        end
    end
end)
rule('brand-is-the-display-name', 'identity', 6, true, function(m)
    if m.from_domain ~= '' and m.display ~= '' and brand_claim(BRANDS, bare_name(m.display), m.from_domain, true) then
        return 'the display name is a company\'s and nothing else, and the domain is not theirs'
    end
end)
rule('impersonates-brand', 'identity', 4.5, false, function(m)
    if m.domain_seen < 3 and m.from_domain ~= '' and m.display ~= ''
        and brand_claim(BRANDS, m.display, m.from_domain, false) then
        return 'display name claims a company, from a domain that is not theirs and is new here'
    end
end)
rule('subject-claims-brand', 'identity', 4.5, false, function(m)
    if m.domain_seen < 3 and trim(m.domain) ~= '' and m.subject_folded ~= ''
        and brand_claim(SUBJECT_CLAIMS, m.subject_folded, trim(m.domain):lower(), false) then
        return 'the subject claims a company, from a domain that is not theirs and is new here'
    end
end)
rule('hostname-claims-company', 'identity', 5, true, function(m)
    for _, b in ipairs(HOSTED_CLAIMS) do
        for label in m.host_prefix:gmatch('[^.]+') do
            if label == b.name and not owns(b.domains, m.host_registrable) then
                return 'the sending host puts `' .. b.name .. '` in front of a domain that is not theirs'
            end
        end
    end
end)
-- A From at our own domain that nothing authenticated as us.  Every
-- rule above starts at `external(m)`, so this is the one case they all
-- decline to look at.
--
-- Three conditions, and the first version had only one.  "Did not
-- authenticate" alone held 62 conversations of this deployment's own
-- system mail: its services submit to the MX without SMTP AUTH, from
-- the container bridge, and some of them are unsigned — `devops@`,
-- `noreply@`, `alias-verify@`, `spf=softfail dkim=none dmarc=fail`.
-- Authentication cannot tell those from a forgery; where they came
-- from can, and a forger cannot borrow our own host's address.
--
-- `dmarc == 'fail'` and not `~= 'pass'`: unknown must decline.  The
-- receiver scans before the stage that checks alignment (it rescans
-- after, which is where this fires), the sweep reads it off the stored
-- header, and neither may guess.  Production has mail from a public
-- address that IS us and says so — `spf=pass dmarc=pass` — and a rule
-- that fired on "not proven" would hold that too.
rule('claims-our-domain', 'identity', 6, true, function(m)
    if not m.unauthenticated or m.peer_is_private then return end
    if m.dmarc ~= 'fail' then return end
    if m.from_domain == '' or not owns(m.our_domains, m.from_domain) then return end
    if owns(m.allowed_domains, m.from_domain) then return end
    return 'the From address is at this organisation\'s own domain, from a public address, and DMARC says it is not us'
end)
-- A From under a name reserved so that it can never be anybody's mail
-- domain: `.invalid`, `.test`, `.example`, `.localhost` (RFC 6761) and
-- `example.com/.net/.org` (RFC 2606).  Nobody can receive a reply there,
-- so nobody with a reason to be answered writes from there.
--
-- Measured over 43,929 production messages on 2026-10-24: 5 new holds,
-- every one a probe somebody sent to see what the MX does with it
-- (`probe@nodmarc-probe.invalid`, `probe@example.org`).  Mail our own
-- containers submit from inside — test fixtures, the outbound mock for
-- `@example.com` — comes from the bridge and is left alone.
local reserved_tlds = {'invalid', 'test', 'example', 'localhost'}
local reserved_domains = {'example.com', 'example.net', 'example.org'}
rule('reserved-sender-domain', 'identity', 6, true, function(m)
    if m.peer_is_private or m.from_domain == '' then return end
    local d = m.from_domain:lower()
    for _, t in ipairs(reserved_tlds) do
        if d == t or d:sub(-(#t + 1)) == '.' .. t then
            return 'the From domain is under `.' .. t .. '`, which is reserved and cannot receive mail'
        end
    end
    for _, e in ipairs(reserved_domains) do
        if d == e or d:sub(-(#e + 1)) == '.' .. e then
            return 'the From domain is `' .. e .. '`, which is reserved and cannot receive mail'
        end
    end
end)
-- The From domain publishes DMARC, and nothing in the message vouches
-- for it: no DKIM signature at all, and SPF did not pass.  A real sender
-- with a DMARC record has at least one of the two; a forger borrowing
-- `gmail.com` or `investing.com` has neither.
--
-- `dkim == 'none'`, not `~= 'pass'`.  A signature that failed to verify
-- is a real sender's mail damaged on the way or a verifier's mistake:
-- the first version counted it, and with `temperror` counted too it
-- held 47 more, among them newsletters and one-time passwords
-- (`epark.jp` through Adobe Campaign, `focusai.com` through Google).  An SPF `temperror` is a DNS outage, not a verdict, so only
-- the four answers that say "not authorised" count.
--
-- Measured over 43,929 production messages on 2026-10-24: 35 new
-- holds, all fraud or probes — loan-funding and compensation scams,
-- shipping and purchase-order lures, a password-expiry phish, brand
-- impersonations the display-name rules already scored, and the
-- tester's `ceo@gmail.com`.
local not_authorised = {fail = true, softfail = true, neutral = true, none = true}
rule('nothing-vouches-for-the-sender', 'identity', 6, true, function(m)
    if m.peer_is_private or not external(m) then return end
    if m.dmarc == 'fail' and m.dkim == 'none' and not_authorised[m.spf] then
        return 'the From domain publishes DMARC, the message carries no DKIM signature, and SPF says `'
            .. m.spf .. '`'
    end
end)
rule('minted-address', 'identity', 6, true, function(m)
    if minted(m.minted_local) and minted(m.minted_sld) then
        return 'neither the mailbox nor the domain is a word anybody chose'
    end
end)
local not_names = {'哈喽','你好','您好','大家好','亲爱的','親愛的','尊敬的','各位','同学','同學',
    '朋友','老师','老師','先生','女士','早上好','下午好','晚上好','恭喜','注意','提醒','通知'}
rule('greets-a-stranger', 'identity', 2, false, function(m)
    local name, n = '', 0
    for _, c in ipairs(chars(trim(m.subject))) do
        if c == ',' or c == '，' then
            if n < 2 or n > 4 then return end
            for _, word in ipairs(not_names) do if name == word then return end end
            if not contains(m.to_display, name) then
                return 'subject greets `' .. name .. '`, which the `To:` header does not name'
            end
            return
        end
        if not is_han(c) then return end
        n = n + 1
        if n > 4 then return end
        name = name .. c
    end
end)
-- A subject that is one word, and the word is a piece of the recipient's
-- own address read as a name: `Hao`, to `lihao@golia.jp`.
--
-- Nobody who knows the reader writes this.  It is what a mail merge
-- produces from a list of bare addresses: split the mailbox, guess a
-- first name, put it in the subject.  The body then opens `Hao, would 4
-- to 8 more high-paying contracts ...` from a domain registered for the
-- purpose — 21 domains in the corpus, one message each, under a handful
-- of recurring personas.
--
-- Measured over 39,685 production messages on 2026-09-23: 21 subjects
-- are a single word cut from the recipient's address, and all 21 are
-- this campaign.
--
-- **Scored, not held.**  It is cold sales, not a fraud on its face, and
-- one legitimate stranger writing `Hao` would be convicted by a hold —
-- so it pushes toward Junk, where the reader still sees it.  5 is the
-- Junk threshold: alone it is enough to leave the inbox, and no more.
--
-- The address is read from `to_display`, which carries it when the
-- `To:` has no display name or uses the address as one (every sample).
-- A `To:` with a real name leaves no address to cut, and the rule
-- declines — the safe direction.
rule('subject-cut-from-the-address', 'identity', 5, false, function(m)
    if not external(m) then return end
    local word = (trim(m.subject):gsub('[,.!?:]+$', ''))
    if not word:match('^[A-Za-z][A-Za-z]+$') then return end
    local mailbox = trim(m.to_display):lower():match('^([a-z0-9._+-]+)@[a-z0-9.-]+$')
    if not mailbox then return end
    word = word:lower()
    if word ~= mailbox and contains(mailbox, word) then
        return 'the subject is only `' .. trim(m.subject) .. '`, a piece of the recipient\'s own address read as a name'
    end
end)
