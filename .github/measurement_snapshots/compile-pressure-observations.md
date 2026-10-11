# Short-case compiler RSS baseline

The compile-pressure table records compiler observations plus explicit headroom.
These values bound build-host compiler processes, not Pico runtime SRAM, stack,
or flash. Runtime ceilings and the compiler guard's enforcement are unchanged.

The original short cases recorded 1 MiB or 4 MiB. Sampling can miss most of a
short-lived compiler process; those values did not describe the Linux ARM host
used by CI. The following actual observations replace only those four baselines,
keeping their existing 128 MiB RSS and 15 second headroom.

| Budget label | Observed RSS | Evidence |
| --- | ---: | --- |
| `route_arm_heavy_1` | 122 MiB | [40494486 CI](https://github.com/hibanaworks/hibana/actions/runs/37242197697), `route-arm-heavy 1`, completed |
| `causal_handoff_4` | 131 MiB | [40494486 CI](https://github.com/hibanaworks/hibana/actions/runs/37242197697), `causal-handoff linear 4`, stopped at the old 129 MiB limit |
| `causal_handoff_route_4` | 129 MiB | [d78ecdea CI](https://github.com/hibanaworks/hibana/actions/runs/37197869136), `causal-handoff route 4`, completed |
| `causal_handoff_roll_4` | 119 MiB | [d78ecdea CI](https://github.com/hibanaworks/hibana/actions/runs/37197869136), `causal-handoff roll 4`, completed |

Both runs use Rust 1.95.0 on `ubuntu-24.04-arm`, compiling the no-std public
choreography/projection cases for `thumbv6m-none-eabi`. The linear-4 observation
is a censored sample, not a claim about the completed compile's peak. The next
uninterrupted CI run must still pass the same time/RSS guard and all resource
and semantic gates. A sampled maximum is not an exact compiler high-water mark.

The successful d78ecdea run already measured linear-4 at 113 MiB, route-4 at
129 MiB, and roll-4 at 119 MiB. Therefore the old 1 MiB baseline was invalid even
before the newer revision. No type explosion is established by the single
131 MiB sample; the larger 64/256-event cases remain independently guarded.

Retrieve the records with `gh run view RUN_ID --log` and select
`compile pressure observed:` or `compile pressure guard violation:` lines.
