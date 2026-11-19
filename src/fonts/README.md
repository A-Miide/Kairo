# Bundled fonts

The faces the slide creator and the built-in timer templates use, kept in the app so a slide looks the same with or
without internet. `fonts.css` declares them; `index.html`, `display.html` and `monitor.html` load it, and
`loadGoogleFont` (in `client_common.js`) asks for these by name instead of fetching them.

| Family | Weights | Used by |
|---|---|---|
| Inter | 100–900 | slide creator body text |
| Montserrat | 100–900 | slide creator (Ember, Lagoon, Sky, Red Card), headings |
| Manrope | 200–800 | interface, timer templates |
| Raleway | 100–900 | slide creator (Lagoon) |
| Space Grotesk | 300–700 | slide creator (Deep Ocean) |
| Playfair Display | 400–900 | slide creator (Midnight), timer templates |
| EB Garamond | 400–800 | slide creator (Evergreen) |
| Cormorant Garamond | 300–700 | slide creator (Linen) |
| Fraunces | 100–900 | slide creator (Sunset, Honey), timer templates |
| Anton | 400 | slide creator (Royal Blue), timer templates |
| Bebas Neue | 400 | slide creator (Crimson Night) |
| Archivo Black | 400 | slide creator (Graphite) |
| DM Serif Display | 400 | slide creator (Forest Mint) |
| Yellowtail | 400 | slide creator script accent |
| Poppins | 400–900 (one file each) | slide creator (Plum Peach, Lavender) |

Subsets: latin and latin-ext (Western European languages, Yoruba and similar). Other scripts fall back to the system font.
Italics are not bundled; an italic is drawn by the browser from the upright face.

Source: Google Fonts, fetched 2026-11-11. Every family is released under the SIL Open Font License 1.1
(https://openfontlicense.org), which allows bundling and redistribution with software. Each family's own copyright
notice is at https://fonts.google.com/ (search the family name).

To add a family, add it to `BUNDLED_FONTS` in `client_common.js` and to `fonts.css` (woff2 from the Google Fonts CSS
API, one `@font-face` per subset).
