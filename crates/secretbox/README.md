# mailrs-secretbox

Sealing a credential with a deployment key.

A refresh token or an app password is as good as the mailbox it opens,
so it is not stored in the clear. This wraps XChaCha20-Poly1305 in the
smallest shape that a store can hold: one string in, one string out,
with the nonce and a key fingerprint carried inside so the key can be
rotated without a migration and so a value sealed under a key that is
gone says exactly that instead of decrypting to noise.

```rust
use mailrs_secretbox::{Key, seal, open};

let key = Key::from_passphrase("whatever the deployment set");
let sealed = seal(&key, b"1//0gLm...")?;      // an opaque string
let plain = open(&key, &sealed)?;             // the token again
# Ok::<(), mailrs_secretbox::Error>(())
```

The sealed form is `v1.<key-fingerprint>.<base64 nonce+ciphertext>`.
The fingerprint is eight bytes of SHA-256 over the key: enough to tell
two keys apart in a store, not enough to attack the key.
