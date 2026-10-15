# Triage Labels

The skills speak in terms of five canonical triage roles. This file maps those roles to the actual label strings used in this repo's issue tracker.

| Label in mattpocock/skills | Label in our tracker | Meaning                                  |
| -------------------------- | -------------------- | ---------------------------------------- |
| `needs-triage`             | `needs-triage`       | Maintainer needs to evaluate this issue  |
| `needs-info`               | `needs-info`         | Waiting on reporter for more information |
| `ready-for-agent`          | `ready-for-agent`    | Fully specified, ready for an AFK agent  |
| `ready-for-human`          | `ready-for-human`    | Requires human implementation            |
| `wontfix`                  | `wontfix`            | Will not be actioned                     |

Default vocabulary, unchanged. Because the tracker is GitHub, these are **real repository labels**, not `Status:` lines in a file — and `gh` will not create them implicitly. Create each once before first use:

```sh
gh label create needs-triage --description "Maintainer needs to evaluate" --color 8277f7
gh label create needs-info --description "Waiting on reporter for more information" --color d4c5f9
gh label create ready-for-agent --description "Fully specified, ready for an agent" --color 0e8a16
gh label create ready-for-human --description "Requires human implementation" --color 1d76db
gh label create wontfix --description "Will not be actioned" --color ffffff
```

`/wayfinder` needs its own set: `wayfinder:map`, `wayfinder:research`, `wayfinder:prototype`, `wayfinder:grilling`, `wayfinder:task`.

When a skill mentions a role (e.g. "apply the AFK-ready triage label"), use the corresponding label string from this table.

Edit the right-hand column to match whatever vocabulary you actually use.
