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
-- An archive whose entries are encrypted but are all documents: the
-- password-in-the-letter habit banks and accountants share with malware.
-- Documents include the legacy Office formats and RTF, which may carry a
-- macro nobody can see while encrypted; the macro-enabled forms (docm…)
-- do not. It scores and never holds; an executable or script entry holds through
-- the rule below, encrypted or not, since an archive's entry names are
-- readable either way. A binary without `has_encrypted_document_archive`
-- reads nil here and fires on nothing (and there encryption alone still
-- sets has_executable_attachment).
rule('encrypted-archive-of-documents', 'content', 3, false, function(m)
    if m.has_encrypted_document_archive == true then
        return 'an attached archive is encrypted; the entries it names are documents'
    end
end)
rule('executable-attachment', 'content', 6, true, function(m)
    if m.has_executable_attachment then return 'carries an attachment the operating system would run' end
end)

-- An attachment whose signature says archive but whose entry names
-- cannot all be read: a 7z, a RAR with header encryption, a RAR whose
-- headers break off or that has more entries than are read.  Nothing
-- in it was checked for a program; when another attachment, or a name
-- that could be read, shows one, the rule above holds and this fact is
-- not set.
--
-- **Scored, not held.**  An archive that will not say what it holds is
-- not proof of anything: ordinary senders send 7z, and header
-- encryption is a setting people choose to keep file names private.
-- 3 is below the Junk threshold (5), so alone it leaves the inbox
-- alone; with another signal it adds up.
rule('archive-names-unreadable', 'content', 3, false, function(m)
    if m.has_unreadable_archive then
        return 'the names of the files inside an attached archive could not be read, so what it holds was not checked'
    end
end)

-- The parse worker died, was killed or ran out of time while reading
-- this message's parts (HTML, attachments, the Bayes tokens), so what
-- those readings would have shown is missing.  A message built to take
-- a parser down is one an attacker made; a mailer's honest oddity can
-- do it too, and the message is delivered either way.
--
-- **Scored, not held**, and asked of every sender: a crash says nothing
-- about who wrote the letter, and a sender cannot step around it by
-- leaving out its `From:`.  3 is below the Junk threshold (5), so alone
-- it leaves the inbox alone; with another signal it adds up.  A worker
-- that was down for its own reasons sets nothing.
rule('parser-failed-on-message', 'content', 3, false, function(m)
    if m.parser_failed_on_message then
        return 'a parser failed while reading this message, so part of it could not be checked'
    end
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
            return found('a link shows the address ' .. shown .. '/… but goes to ' .. target,
                {shown = shown, target = target})
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
        return found('the subject names the recipient\'s own address, ' .. address, {address = address})
    end
end)
-- An HTML or SVG attachment built to do something a page has no business
-- doing in mail: assemble a file in the browser and make it save it
-- (HTML smuggling), run script from an image, or post a password to a
-- host that is not the sender's.  All three are the attachment's own
-- structure, read as bytes by `mailrs_fraud::html_attachment`; nothing
-- here depends on what the letter says or who sent it.
rule('html-builds-a-download', 'content', 6, true, function(m)
    if m.html_builds_a_download then
        return 'an attached page assembles a file from data it carries and makes the browser save it'
    end
end)
rule('svg-runs-script', 'content', 6, true, function(m)
    if m.svg_runs_script then return 'an attached image runs script' end
end)
rule('attachment-is-a-login-page', 'content', 6, true, function(m)
    if m.attachment_login_form_offsite then
        return "an attached page asks for a password and sends it to another site than the sender's"
    end
end)
-- Where a QR code in an image or a scanned page goes, against what the
-- message says.  The codes are read after delivery, so these only ever
-- fire when stored mail is judged again; the live path has no codes.
--
-- **Shadow**: hits are counted and recorded, nothing is scored or held,
-- until the measured hits on real mail say what they are worth.  An
-- image printing one domain while its code goes to another is the
-- strongest; a code as the only way out of a letter with no links, to a
-- host the letter never names, or to a domain new here, are weaker:
-- receipts and tickets carry codes too.
--
-- `qr-text-says-another-domain` is to be reviewed on 2026-12-07: its
-- first measurement hit two conversations, one that reads as a scam and
-- one an estate agent's letter, too few to decide on.
rule('qr-text-says-another-domain', 'content', 6, false, function(m)
    if external(m) and m.qr_text_mismatch then
        return found('an image prints one domain while its QR code goes to another',
            {domains = m.qr_domains})
    end
end, {shadow = true})
rule('qr-only-way-out', 'content', 3, false, function(m)
    if external(m) and m.qr_codes > 0 and not m.has_links and m.qr_host_not_in_message then
        return found('no links, only a QR code to a host the message never names',
            {domains = m.qr_domains})
    end
end, {shadow = true})
rule('qr-to-a-new-domain', 'content', 3, false, function(m)
    if external(m) and m.qr_domain_new and m.qr_host_not_in_message then
        return found('a QR code to a domain first seen here this week that the message never names',
            {domain = m.new_qr_domain})
    end
end, {shadow = true})
