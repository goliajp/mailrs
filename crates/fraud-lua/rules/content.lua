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
