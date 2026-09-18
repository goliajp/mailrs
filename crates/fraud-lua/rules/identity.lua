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
