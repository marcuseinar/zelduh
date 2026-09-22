# Vendored libraries

Third-party code, checked in rather than installed, so that the build stays
`cargo build` and the page keeps working offline with no package manager,
bundler or CDN in the picture.

| File | Version | Licence | Upstream |
| --- | --- | --- | --- |
| `nipplejs.js` | 0.10.2 | MIT | <https://github.com/yoannmoinet/nipplejs> |
| `trystero/` | 0.25.4 | MIT | <https://github.com/dmotz/trystero> |
| `trystero/secp256k1.mjs` | 3.1.0 | MIT | <https://github.com/paulmillr/noble-secp256k1> |
| `jsfxr/sfxr.js` | 1.2.1 | Unlicense | <https://github.com/chr15m/jsfxr> |

Each file is the unmodified published build, with only its CDN banner replaced
by a one-line attribution, and — in Trystero's case — the bare module
specifiers it imports rewritten to the relative paths they sit at here.
`jsfxr/sfxr.js` is `riffwave.js` and `sfxr.js` concatenated, which is how that
package expects to be loaded in a browser.
