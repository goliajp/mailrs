-- Attachment rules. `junk` puts the message in Junk with `reason` shown to
-- the reader; `score` adds to the message's score and reaches Junk only
-- with other signals. Ideas are studied from the projects named in
-- `inspired_by`; nothing is copied from them.

local function on_part(spec, field, outside_only)
    spec.kind = 'attachment'
    spec.check = function(m)
        local p = part_with(field, outside_only)
        if p then return p.name .. ': ' .. p[field], part_cite(p) end
    end
    rule(spec)
end

on_part({ id = 'exe-name', action = 'junk', score = 0,
    reason = 'It carries a program or script that would run when opened.',
    inspired_by = "rspamd MIME_BAD_EXTENSION; Outlook's blocked attachment list" },
    'executable_name', true)

on_part({ id = 'double-extension', action = 'junk', score = 0,
    reason = 'Its file name hides what the file really is.',
    inspired_by = 'rspamd MIME_DOUBLE_BAD_EXTENSION; Unicode right-to-left override abuse' },
    'double_extension', false)

on_part({ id = 'type-mismatch', action = 'junk', score = 0,
    reason = 'The file is a program or disk image while its name says otherwise.',
    inspired_by = 'ClamAV file-type detection' },
    'type_mismatch', false)

on_part({ id = 'archive-executable', action = 'junk', score = 0,
    reason = 'An archive in it holds a program or script that would run when opened.',
    inspired_by = 'rspamd archive checks' },
    'archive_executable', true)

-- Encrypted zips with the password sent separately are common in Japanese
-- business mail, so this is only a score.
on_part({ id = 'encrypted-archive', action = 'score', score = 2,
    reason = 'It carries a password-protected archive, which no check can look inside.',
    inspired_by = 'ClamAV Heuristics.Encrypted.Zip' },
    'encrypted_archive', true)

on_part({ id = 'unlistable-archive', action = 'score', score = 1,
    reason = 'It carries a rar or 7z archive whose contents are not checked.',
    inspired_by = 'rspamd archive module' },
    'unlistable_archive', true)

on_part({ id = 'office-macros', action = 'junk', score = 0,
    reason = 'It carries an Office document with macros, which can run code when opened.',
    inspired_by = "Microsoft's default blocking of macros from the internet; ClamAV Heuristics.OLE2.ContainsMacros" },
    'office_macros', true)

on_part({ id = 'office-object', action = 'score', score = 2,
    reason = 'It carries an Office document with an embedded object.',
    inspired_by = 'oletools oleobj' },
    'office_object', true)

on_part({ id = 'rtf-object', action = 'junk', score = 0,
    reason = 'It carries an RTF document with an embedded object, a common way to deliver an exploit.',
    inspired_by = 'oletools rtfobj; RTF exploits such as CVE-2017-11882' },
    'rtf_object', true)

on_part({ id = 'pdf-script', action = 'junk', score = 0,
    reason = 'It carries a PDF that runs a script, launches a program, or embeds one.',
    inspired_by = "Didier Stevens' pdfid" },
    'pdf_script', false)

-- E-invoices (Factur-X, ZUGFeRD) embed XML in a PDF, so this is only a score.
on_part({ id = 'pdf-embedded', action = 'score', score = 0.5,
    reason = 'It carries a PDF with an embedded file or an action on opening.',
    inspired_by = "Didier Stevens' pdfid" },
    'pdf_embedded', false)

on_part({ id = 'html-form', action = 'junk', score = 0,
    reason = 'It carries a web page with a form that sends what you type to another site.',
    inspired_by = 'credential phishing kits delivered as HTML attachments' },
    'html_form', true)

on_part({ id = 'svg-script', action = 'junk', score = 0,
    reason = 'It carries a picture that runs script.',
    inspired_by = 'SVG phishing campaigns' },
    'svg_script', false)

rule({ id = 'html-smuggling', kind = 'attachment', action = 'junk', score = 0,
    reason = 'It carries a web page that builds a file inside the browser, a way to get past checks.',
    inspired_by = 'published HTML smuggling write-ups (Microsoft, Proofpoint)',
    check = function(m)
        for _, p in ipairs(m.parts) do
            if p.html_writer and p.html_base64_run >= policy.html_smuggling_min_base64 then
                return p.name .. ': ' .. p.html_writer .. ' over ' .. p.html_base64_run .. ' base64 bytes', part_cite(p)
            end
        end
    end })

rule({ id = 'own-hash-listed', kind = 'attachment', action = 'junk', score = 0,
    reason = 'The file is one already found to be malicious here.',
    inspired_by = 'local hash lists in ClamAV and rspamd',
    check = function(m)
        for _, p in ipairs(m.parts) do
            local h = p.hash_listed
            if h then
                local ev = part_cite(p)
                ev[2] = h.id .. ' (' .. h.age_days .. ' days): ' .. h.reason:sub(1, 200)
                return p.name .. ' matches ' .. h.id, ev
            end
        end
    end })
