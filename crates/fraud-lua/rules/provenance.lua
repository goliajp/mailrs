local vendors = {'outlook','thunderbird','apple','iphone','ipad','mail','zimbra','roundcube','php',
    'sendgrid','mailchimp','amazon','postfix','exim','zoho','gmail','yahoo','becky','shuriken','edmax',
    'salesforce','marketo','sendinblue','klaviyo','mailer','smtp','python','ruby','java','node','swift',
    'sparkpost','mandrill','system','server','notes','groupware','cybozu','desknet','sakura','xserver'}
rule('x-mailer-generated', 'provenance', 5, true, function(m)
    local v = trim(m.x_mailer)
    if #v == 0 or #v > 120 then return end
    for _, vendor in ipairs(vendors) do if contains(v:lower(), vendor) then return end end
    local parts = words(v)
    if #parts < 2 or #parts > 4 then return end
    for i = 1, #parts-1 do
        if #parts[i] < 4 or not parts[i]:match('^[a-zA-Z]+$') then return end
    end
    local numeric = parts[#parts]
    if not contains(numeric, '.') then return end
    for g in (numeric .. '.'):gmatch('(.-)%.') do
        if #g == 0 or #g > 6 or not g:match('^[0-9]+$') then return end
    end
    return found('X-Mailer is one no mail client writes', {mailer = v})
end)
rule('reply-domain-rotation', 'provenance', 6, true, function(m)
    if m.reply_rotation >= 4 then
        return found('replies go to a domain that ' .. m.reply_rotation .. ' different sending domains use',
            {count = tostring(m.reply_rotation)})
    end
end)
rule('minted-sending-host', 'provenance', 3, false, function(m)
    local label = m.host_prefix:match('[^.]+')
    if label and #label >= 5 and #label <= 12 and label:match('^[a-z0-9]+$')
        and label:match('[0-9]') and vowels(label) / #label < 0.3 then
        return found('the sending host\'s name was generated, not chosen', {host = m.host_prefix})
    end
end)
-- A `Date:` days ahead of arrival sorts the letter above everything that
-- comes after it, which is the point of writing it. A mis-set clock or a
-- mislabelled zone is off by hours, at most a day (UTC+14 against
-- UTC-12): of 43,072 production letters three were ahead by more than an
-- hour, a county notice by seven, and only this kind by more than one day.
rule('dated-in-the-future', 'provenance', 6, true, function(m)
    if m.date_ahead > 2 * 86400 then
        local days = math.floor(m.date_ahead / 86400)
        return found('dated ' .. days .. ' days after it arrived, so it sorts above every letter that follows',
            {days = tostring(days)})
    end
end)
-- Business email compromise, by structure only.  "Pay us" and "our bank
-- details changed" are words; what a message can prove is a payment
-- identifier whose own check digits pass (an IBAN, a Bitcoin address)
-- and the reader's history with the sender: a first contact, a domain a
-- letter or two away from one the reader writes to, a correspondent's
-- name at an address that has never written, a reply sent elsewhere.
--
-- **Scored, never held.**  Every one of these leans on the reader's
-- history, and familiarity convicts nobody: a new supplier's first
-- invoice carries an IBAN too.  Together they can reach the Junk
-- threshold; none of them alone can hide mail.
local function pays(m) return m.has_iban or m.has_crypto_address end
rule('bec-pay-to-a-stranger', 'provenance', 3, false, function(m)
    if pays(m) and m.first_contact then
        return found('the first message from this address carries a payment account',
            {account = account_of(m), domain = m.from_domain})
    end
end)
rule('bec-pay-to-a-lookalike', 'provenance', 4, false, function(m)
    if pays(m) and m.looks_like_correspondent then
        return found('a payment account, from a domain one or two letters away from one you write to',
            {account = account_of(m), domain = m.from_domain, like = m.lookalike_of})
    end
end)
rule('bec-name-borrowed', 'provenance', 4, false, function(m)
    if m.name_borrowed and (m.reply_to_outside or pays(m)) then
        return found("a correspondent's name at an address that has never written to you, sending replies elsewhere or carrying a payment account",
            {domain = m.from_domain})
    end
end)
rule('bec-lookalike-reply-diverted', 'provenance', 3, false, function(m)
    if m.looks_like_correspondent and m.reply_to_outside then
        return found('from a domain one or two letters away from one you write to, with replies sent to another domain',
            {domain = m.from_domain, like = m.lookalike_of})
    end
end)
rule('bec-reply-diverted-unauthenticated', 'provenance', 4, false, function(m)
    if m.from_correspondent_domain and m.reply_to_outside
        and m.dmarc ~= '' and m.dmarc ~= 'pass' then
        return found("claims a domain you write to without passing that domain's DMARC, and sends replies elsewhere",
            {domain = m.from_domain})
    end
end)
