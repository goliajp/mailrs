local function chars(s)
    local out = {}
    for c in s:gmatch('[%z\1-\127\194-\244][\128-\191]*') do out[#out+1] = c end
    return out
end
local function words(s)
    local out = {}
    for w in spaces(s):gmatch('[^ ]+') do out[#out+1] = w end
    return out
end
-- Unicode normalization and address parsing are host facts; decisions live here.
local function contains(s, needle) return s:find(needle, 1, true) ~= nil end
local function owns(domains, domain)
    for _, d in ipairs(domains) do
        if domain == d or domain:sub(-#d - 1) == '.' .. d then return true end
    end
    return false
end
local function external(m)
    return m.from_domain ~= '' and not owns(m.our_domains, m.from_domain)
        and not owns(m.allowed_domains, m.from_domain)
end
local function bare_name(s)
    local out, depth = {}, 0
    for _, c in ipairs(chars(s)) do
        if contains('(（[［【', c) then depth = depth + 1
        elseif contains(')）]］】', c) then depth = math.max(0, depth - 1)
        elseif depth == 0 and not contains('®™©・|/', c) then out[#out+1] = c end
    end
    return table.concat(out)
end
local function subject_rest(s)
    local out = {}
    for _, c in ipairs(chars(s)) do
        if not contains('()（）[]［］【】〔〕《》:：.。,、-_|｜/／!！?？~～*#＃><＞＜·・', c) then
            out[#out+1] = c
        end
    end
    s = table.concat(out)
    while true do
        if s:sub(1,2) == 're' then s = s:sub(3)
        elseif s:sub(1,3) == 'fwd' then s = s:sub(4)
        elseif s:sub(1,2) == 'fw' then s = s:sub(3)
        else return s end
    end
end
local function vowels(s)
    local _, n = s:gsub('[aeiou]', '')
    return n
end
local function minted(s)
    return #s >= 5 and #s <= 12 and s:match('^[a-z]+$') and vowels(s) / #s <= 0.2
end
local function brand_claim(list, text, domain, exact)
    for _, b in ipairs(list) do
        local hit = exact and text == fold(b.name) or not exact and contains(text, fold(b.name))
        if exact then
            for _, d in ipairs(b.domains) do hit = hit or text == fold(d) end
        end
        if hit and not owns(b.domains, domain) then return true end
    end
    return false
end
-- Helpers are lexical upvalues of the rule modules in the concatenated bundle.
