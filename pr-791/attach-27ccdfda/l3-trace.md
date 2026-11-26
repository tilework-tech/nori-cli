# `@` on a joined `nori attach --host`, separate host and client homes

Host folder has `HostOnlyQuartz7c.txt`; sessions amber and birch each hold one unique file; the client folder holds `ClientOnlyFern9.txt`. Requests and replies are read off the wire by a relay between client and host.

### fc7324c2

| step | typed | session shown | host request → root, paths | popup |
|---|---|---|---|---|
| no session | `@HostOnlyQ` | (no session) | (none: host folder) → hostdir ['HostOnlyQuartz7c.txt'] | HostOnlyQuartz7c.txt |
| no session | `@AmberOnlyL` | (no session) | (none: host folder) → hostdir [] | no matches |
| no session | `@ClientOnlyF` | (no session) | (none: host folder) → hostdir [] | no matches |
| no session, same text again | `@HostOnlyQ` | (no session) | (none: host folder) → hostdir ['HostOnlyQuartz7c.txt'] | HostOnlyQuartz7c.txt |
| right after Alt+S to birch | `@BirchOnlyW` | birch | birch → birch ['BirchOnlyWren5.rs'] | BirchOnlyWren5.rs |
| right after Alt+S to birch | `@AmberOnlyL` | birch | birch → birch [] | no matches |
| birch, same text again | `@BirchOnlyW` | birch | birch → birch ['BirchOnlyWren5.rs'] | BirchOnlyWren5.rs |
| right after Alt+S to amber | `@BirchOnlyW` | amber | amber → amber [] | no matches |
| right after Alt+S to amber | `@AmberOnlyL` | amber | amber → amber ['AmberOnlyLark3.rs'] | AmberOnlyLark3.rs |
| amber, same text again | `@AmberOnlyL` | amber | **not sent** | loading... |
| right after Alt+S to birch | `@BirchOnlyW` | birch | birch → birch ['BirchOnlyWren5.rs'] | BirchOnlyWren5.rs |
| right after Alt+S to birch | `@AmberOnlyL` | birch | birch → birch [] | no matches |
| birch, same text again | `@BirchOnlyW` | birch | birch → birch ['BirchOnlyWren5.rs'] | BirchOnlyWren5.rs |

### new1

| step | typed | session shown | host request → root, paths | popup |
|---|---|---|---|---|
| no session | `@HostOnlyQ` | (no session) | (none: host folder) → hostdir ['HostOnlyQuartz7c.txt'] | HostOnlyQuartz7c.txt |
| no session | `@AmberOnlyL` | (no session) | (none: host folder) → hostdir [] | no matches |
| no session | `@ClientOnlyF` | (no session) | (none: host folder) → hostdir [] | no matches |
| no session, same text again | `@HostOnlyQ` | (no session) | (none: host folder) → hostdir ['HostOnlyQuartz7c.txt'] | HostOnlyQuartz7c.txt |
| right after Alt+S to birch | `@BirchOnlyW` | birch | birch → birch ['BirchOnlyWren5.rs'] | BirchOnlyWren5.rs |
| right after Alt+S to birch | `@AmberOnlyL` | birch | birch → birch [] | no matches |
| birch, same text again | `@BirchOnlyW` | birch | birch → birch ['BirchOnlyWren5.rs'] | BirchOnlyWren5.rs |
| right after Alt+S to amber | `@BirchOnlyW` | amber | amber → amber [] | no matches |
| right after Alt+S to amber | `@AmberOnlyL` | amber | amber → amber ['AmberOnlyLark3.rs'] | AmberOnlyLark3.rs |
| amber, same text again | `@AmberOnlyL` | amber | amber → amber ['AmberOnlyLark3.rs'] | AmberOnlyLark3.rs |
| right after Alt+S to birch | `@BirchOnlyW` | birch | birch → birch ['BirchOnlyWren5.rs'] | BirchOnlyWren5.rs |
| right after Alt+S to birch | `@AmberOnlyL` | birch | birch → birch [] | no matches |
| birch, same text again | `@BirchOnlyW` | birch | birch → birch ['BirchOnlyWren5.rs'] | BirchOnlyWren5.rs |
