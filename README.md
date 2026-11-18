# nori-cli Cliff co-author candidate (2026-11-18)

Scope: 19 squash commits on tilework-tech/nori-cli main, e81d88912 (exclusive) .. bc6681685 (main tip at prep time).
PRs: 647 648 649 651 652 653 654 655 656 657 658 659 660 661 662 664 665 666 668 (merged 2026-11-15..2026-11-18).
Change: append `Co-authored-by: Clifford Ressel <6520751+CSRessel@users.noreply.github.com>` as the last trailer line. Nothing else changed except parent SHAs and the removed GitHub web-flow gpgsig (cannot be reproduced).

- Candidate branch: origin/review/cliff-coauthor-history-2026-11-18 = f67230736cba5cb2f92c7686eba337f2f023e709
- Old main tip: bc6681685f3bc10f3f11dbffb61b7f9f0709ef83 (tree 1f5b491c..., identical in new tip)
- Map: sha-map.tsv (verify column all ok). Scripts: scripts/rewrite.py, scripts/verify.py
- Bundle: candidate.bundle (refs review/..., backup/main-before-coauthor; prerequisite e81d88912), sha256 7a3063241c5b2b8ec67f072f313b9ce1501b475f6b27e98f0035ef755283d078

Operation needing Cliff's confirmation (not done):
  git push --force-with-lease=refs/heads/main:bc6681685f3bc10f3f11dbffb61b7f9f0709ef83 origin f67230736cba5cb2f92c7686eba337f2f023e709:refs/heads/main
Recovery: git push --force-with-lease=refs/heads/main:f67230736cba5cb2f92c7686eba337f2f023e709 origin bc6681685f3bc10f3f11dbffb61b7f9f0709ef83:refs/heads/main
(old commits stay reachable on GitHub via tags nori-v0.29.0-next.13..nori-v0.29.1-next.8 and PR refs; also in candidate.bundle)
