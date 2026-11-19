# Blackmagic DeckLink API headers (Desktop Video SDK 16.0)

The `include` folders of Blackmagic Design's Desktop Video SDK 16.0, unmodified:
`mac/` (Mac/Include), `linux/` (Linux/Include), `win/` (Win/Include — IDL,
turned into headers by MIDL at build time). Clause 0.1 of the SDK's End User
License Agreement exempts these folders from its restrictive clauses, and
each file carries Blackmagic's own permissive licence notice, which must stay.

Used only by `src-tauri/native/decklink.cpp`. The DeckLink driver library is
not included: it comes with Blackmagic Desktop Video, installed by anyone who
has a DeckLink card. "Kairo compatible with Blackmagic Design DeckLink".
