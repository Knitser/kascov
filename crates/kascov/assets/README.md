# Embedded font assets

These TTFs are embedded into the `kascov` worker binary (`include_bytes!` in
`src/og.rs`) so the Cloud Run debian-slim runtime — which ships no fonts —
can render the `/og/{network}/{id}.png` share cards.

| File | Family | Source | License |
| --- | --- | --- | --- |
| `SpaceGrotesk-Bold.ttf` | Space Grotesk (700) | https://github.com/floriankarsten/space-grotesk (`fonts/ttf/static/`) | SIL Open Font License 1.1 |
| `JetBrainsMono-Regular.ttf` | JetBrains Mono (400) | https://github.com/JetBrains/JetBrainsMono (`fonts/ttf/`) | SIL Open Font License 1.1 |

Both fonts are licensed under the SIL Open Font License, Version 1.1
(https://openfontlicense.org). The OFL permits bundling, redistribution and
embedding; the fonts themselves are not sold and retain their original names
and copyright:

- Space Grotesk — Copyright 2020 The Space Grotesk Project Authors
  (https://github.com/floriankarsten/space-grotesk)
- JetBrains Mono — Copyright 2020 The JetBrains Mono Project Authors
  (https://github.com/JetBrains/JetBrainsMono)
