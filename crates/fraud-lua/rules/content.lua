rule('zero-width-name', 'identity', 4.5, true, function(m)
    if m.has_zero_width_in_name then return 'the display name has invisible characters spliced into it' end
end)
rule('bidi-display-name', 'identity', 4.5, true, function(m)
    if m.has_bidi_override then return 'the display name is reordered as it renders — it shows one thing and says another' end
end)
rule('zero-width-inside-a-word', 'content', 6, true, function(m)
    if m.has_zero_width_inside_a_word then
        return 'invisible characters are wedged inside a word, which defeats a filter while rendering unchanged to the reader'
    end
end)
rule('executable-attachment', 'content', 6, true, function(m)
    if m.has_executable_attachment then return 'carries an attachment the operating system would run' end
end)
-- A letter that offers its reader part of a sum in the millions: the
-- advance-fee fraud.  It has no header of its own — the wave that
-- reached an inbox on 2026-09-28 came from a real company's stolen
-- account with SPF, DKIM and DMARC passing — so the offer in the text
-- is the whole case.  `mailrs_fraud::advance_fee` says what counts.
--
-- Measured over 40,866 production messages on 2026-09-28: 9 caught,
-- all fraud from five campaigns, none wrong.
--
-- **Scored, not held.**  The phrases are English and the sample is
-- nine letters; a genuine estate lawyer or investment proposal can say
-- the same words.  5 is the Junk threshold, so it leaves the inbox on
-- its own and stays where the reader can see it.
--
-- Mailing-list mail is excused: newsletters report other people's
-- millions with a percentage beside them.  Most bulk mail carries no
-- list header at all, so this only ever releases.
rule('offers-the-reader-a-sum', 'content', 5, false, function(m)
    if m.offers_the_reader_a_sum and not m.is_bulk then
        return 'the text offers the reader a share of a sum in the millions'
    end
end)
-- A letter that asks its reader to send over their own LINE QR code.
-- The business-email compromise aimed at this company asks for nothing
-- else in the letter; the money is asked for later, in a chat no mail
-- filter sees.  Persona, company, sending domain and mailer all change
-- from wave to wave, and this request does not.
-- `mailrs_fraud::line_handoff` says what counts.
--
-- Measured over 43,838 production messages on 2026-10-23: 263 caught,
-- all this campaign, nothing else.  Eight of them no other rule held:
-- outlook.com and hotmail.com senders, where no address or domain rule
-- has anything to read.
--
-- **Held.**  The request is the fraud itself, not a hint about the
-- sender, and a held letter still reaches review.
rule('asks-for-the-readers-line', 'content', 6, true, function(m)
    if m.asks_for_the_readers_line then
        return 'the text asks the reader to send their own LINE QR code'
    end
end)
-- A link whose text is an address on one of our own domains, with a
-- path, while the link goes somewhere else.  Reported 2026-10-27: `IT
-- Security Officer <doc@wbg-ae.com>`, "Password Reminder for
-- lihao@golia.jp", with the text `www. golia.jp /lihao@golia.jp/
-- check-activity/` linking to a fake webmail login.  The sender's own
-- domain passes SPF and DKIM, so the link is the whole case.
-- `mailrs_fraud::links` says what the text shows and where it goes.
--
-- Measured over 43,854 production messages on 2026-10-27: 1 caught,
-- that message.  A bare `golia.jp` in link text matched 22, most of
-- them Google's business report naming the company; the path is what
-- makes the text a claim about where the link goes.
--
-- **Held.**  Our own address shown over a link to somewhere else is
-- the deception itself, and a held letter still reaches review.  Only
-- external senders: our own mail may well link through a tracker.
rule('link-shows-our-address', 'content', 6, true, function(m)
    if not external(m) then return end
    for shown, target in m.shown_links:gmatch('([^ \n]+) ([^\n]+)') do
        if owns(m.our_domains, shown) and not owns(m.our_domains, target) then
            return 'a link shows the address ' .. shown .. '/… but goes to ' .. target
        end
    end
end)
-- A link still carrying a phishing kit's template call, `[[random_
-- digits(8)]]`, that the sending script never filled in.  Same message
-- as above; 1 caught over 43,854, that one.  Wider template shapes
-- (`{{x}}`, `*|X|*`) matched 170, mostly broken newsletter templates.
--
-- **Scored, not held.**  One sample is a fingerprint of one kit, not a
-- measurement; 5 is the Junk threshold, so it leaves the inbox alone.
rule('link-kit-placeholder', 'content', 5, false, function(m)
    if external(m) and m.link_has_kit_placeholder then
        return 'a link still carries the unfilled template of a phishing kit'
    end
end)
-- A subject that carries the recipient's own full address: "Password
-- Reminder for lihao@golia.jp", "Attention! takagi@golia.jp",
-- "Support Service: lihao@golia.jp Password Expiry Alert!".
--
-- Measured over 43,854 production messages on 2026-10-27: 8 caught,
-- all credential phishing, nothing else.
--
-- **Scored, not held.**  Eight letters is a small sample, and a real
-- service can name the account it writes about.  5 is the Junk
-- threshold: alone it leaves the inbox; with any other signal it adds
-- up.  The address is read from `to_display`, which carries it when the
-- `To:` has no display name or uses the address as one; a real name
-- there leaves nothing to compare, and the rule declines.
rule('subject-names-the-recipient', 'identity', 5, false, function(m)
    if not external(m) then return end
    local address = trim(m.to_display):lower():match('^([a-z0-9._+-]+@[a-z0-9.-]+%.[a-z]+)$')
    if address and contains(m.subject:lower(), address) then
        return 'the subject names the recipient\'s own address, ' .. address
    end
end)
