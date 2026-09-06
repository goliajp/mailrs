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
    return 'X-Mailer is one no mail client writes'
end)
rule('reply-domain-rotation', 'provenance', 6, true, function(m)
    if m.reply_rotation >= 4 then
        return 'replies go to a domain that ' .. m.reply_rotation .. ' different sending domains use'
    end
end)
rule('minted-sending-host', 'provenance', 3, false, function(m)
    local label = m.host_prefix:match('[^.]+')
    if label and #label >= 5 and #label <= 12 and label:match('^[a-z0-9]+$')
        and label:match('[0-9]') and vowels(label) / #label < 0.3 then
        return 'the sending host\'s name was generated, not chosen'
    end
end)
