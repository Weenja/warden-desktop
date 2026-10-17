# Warden browser extension (staging)

Chrome extension id (staging, fixed by the public `key` in the manifest):

    blcmgpbfbbemkkcfjgbhgblnapbndfgp

Native messaging origin for the desktop host manifest: `chrome-extension://blcmgpbfbbemkkcfjgbhgblnapbndfgp/`
Firefox extension id (staging): `extension-staging@getwarden.app`
Native host name (staging): `in.weenja.warden.staging`

The private key that matches the id is kept outside the repo at `~/.config/warden/extension-staging-key.pem`
(only needed to pack a `.crx`; loading unpacked uses the public key in the manifest).

Build and install steps follow when the extension is built.
