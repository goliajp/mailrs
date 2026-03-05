-- supermode: allow an account to view emails across multiple domains
ALTER TABLE accounts ADD COLUMN IF NOT EXISTS super_domains TEXT NOT NULL DEFAULT '';

-- example: grant supermode to lihao@golia.jp for golia.jp and dadaya.jp
-- UPDATE accounts SET super_domains = 'golia.jp,dadaya.jp' WHERE address = 'lihao@golia.jp';
