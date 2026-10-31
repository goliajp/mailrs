-- Shared by the rules below. Nothing here decides anything.

local function cut(s)
    if #s > 200 then return s:sub(1, 200) end
    return s
end

-- The listings from `sources` (a set of source names) still inside their
-- source's lifetime.
function live(list, sources)
    local out = {}
    for _, l in ipairs(list) do
        if sources[l.source] then
            local life = policy.lifetime_days[l.source]
            if life == nil or l.age_days <= life then out[#out + 1] = l end
        end
    end
    return out
end

-- One evidence line per listing: the observation, its source, its age.
function cite(list)
    local out = {}
    for i, l in ipairs(list) do
        if i > 20 then break end
        out[#out + 1] = l.id .. ' (' .. l.source .. ', ' .. l.age_days .. ' days): ' .. cut(l.reason)
    end
    return out
end

-- Marks of `source` on one domain inside `window` days: how many, from how
-- many distinct people, and the evidence lines.
function recent(marks, source, window)
    local n, people, seen, ev = 0, 0, {}, {}
    for _, x in ipairs(marks.marks) do
        if x.source == source and x.age_days <= window then
            n = n + 1
            if x.actor ~= '' and not seen[x.actor] then
                seen[x.actor] = true
                people = people + 1
            end
            if #ev < 20 then ev[#ev + 1] = x.id .. ' (' .. x.age_days .. ' days)' end
        end
    end
    return n, people, ev
end

-- Whether marks of `source` on a domain cross the policy's thresholds.
-- A domain on the never-list never does.
function crosses(marks, source)
    if marks == nil or marks.never then return end
    local cfg = policy[source]
    local n, people, ev = recent(marks, source, cfg.window_days)
    if people >= cfg.min_reporters and n >= cfg.min_marks then
        return marks.domain .. ': ' .. n .. ' marks from ' .. people .. ' people in ' .. cfg.window_days .. ' days', ev
    end
end

-- The first attachment that has `field`; nil for mail from this server's
-- own domains when `outside_only`.
function part_with(field, outside_only)
    if outside_only and not m.outside then return end
    for _, p in ipairs(m.parts) do
        if p[field] then return p end
    end
end

function part_cite(p)
    return { 'part ' .. p.part .. ' ' .. cut(p.name) .. ' sha256 ' .. p.sha256 }
end
