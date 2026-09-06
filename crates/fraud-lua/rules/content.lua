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
