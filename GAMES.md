# Demo Game Evaluation — what to build on top of freshet

> Project renamed **Cascade → `freshet`** (2026-06-16). "Cascade" below = `freshet`;
> the hero demo crate is `freshet-royale`.

The demo is the visibility vehicle. It must (a) be **impossible without Cascade** —
i.e. a single event must modify *more than ~128 accounts*, **push-mode**, where idle
players still get affected (otherwise pull-based would suffice and the primitive
isn't needed); (b) grab attention; (c) ideally work on **mobile** (Solana Mobile /
mobile web), which favors simple, async/turn-based, tap-first interactions and push
notifications.

## Scoring (1–5, higher = better)

| Game | Needs the primitive | Virality | Mobile fit | Build effort (5=easy) | Originality | **Total** |
|---|:--:|:--:|:--:|:--:|:--:|:--:|
| **Elimination Royale** ("Last One Standing") | 5 | 5 | 5 | 4 | 3 | **22** |
| **Critter World** (global-event creatures) | 5 | 4 | 5 | 3 | 4 | **21** |
| **Pixel War** (on-chain r/place + tick) | 4 | 5 | 4 | 3 | 3 | **19** |
| **On-chain Game of Life** (world ticks) | 5 | 3 | 3 | 4 | 4 | **19** |
| **Tournament Auto-Resolver** (brackets) | 4 | 2 | 3 | 4 | 2 | **15** |
| **Mass Raffle / Survival Pool** | 3 | 3 | 4 | 5 | 2 | **17** |

---

## Top picks

### 🥇 Elimination Royale — "Last One Standing"
A large lobby (hundreds–thousands) of players. Each **round**, a global event
eliminates everyone who didn't meet a condition (didn't tap in time, picked the
"wrong door," stood on the shrinking safe zone, etc.). The eliminations are a single
logical event that must update **every** affected player account — **including idle
players who never opened the app** — which is exactly push-mode fan-out that
pull-based cannot express. Survivors split the pot; payout is a second Cascade
effect.

- **Why it showcases Cascade perfectly**: idle players *must* be eliminated on-chain;
  there is no "come claim your elimination." Round resolution = one `Effect`, cranked
  in batches, fully verifiable (no trusted server deciding who dies).
- **Virality**: the Squid-Game / battle-royale format is proven viral; "fully
  on-chain, provably fair elimination" is a strong hook.
- **Mobile**: dead-simple — one tap per round, async, push notification "Round 7
  starts in 60s." Ideal for Solana Mobile / mobile web.
- **Build effort**: moderate — round state machine + Cascade for resolution + a thin
  reactive UI. The hard part (fan-out) is Cascade's job.

### 🥈 Critter World — global-event creature collective
Every player owns on-chain creatures (PDA-by-index entities). Periodic **world
events** (weather, plague, migration, season change) modify *all* creatures at once —
stats decay, mutations, mass breeding — whether or not the owner is online.

- **Why it showcases Cascade**: the world ticks for everyone; offline creatures still
  change. Push-mode, unbounded set. Pull-based fails because the *world* must advance
  independently of player presence.
- **Retention angle**: recurring world events bring players back daily — better
  long-term engagement than a one-shot royale.
- **Mobile**: collection/tamagotchi loop is mobile-native; notifications drive
  re-engagement.
- **Build effort**: higher (creature system + art), but the strongest *product* (not
  just a demo).

---

## The others (why they rank lower)

- **Pixel War** (on-chain r/place): very viral and mobile-friendly (tap a pixel), and
  a periodic "tick" (decay / reward contributors) is real push-mode fan-out. But the
  fan-out is somewhat optional (could be pull-based reward claims), slightly weakening
  the "impossible without Cascade" argument. Strong backup.
- **On-chain Game of Life / cellular automata world**: every tick updates thousands
  of cells — a *flawless* technical showcase and visually mesmerizing. But it reads as
  art/sim more than game; lower virality and weaker mobile interaction. Great as a
  secondary "tech flex" demo alongside a game.
- **Tournament Auto-Resolver**: legitimate fan-out (resolve many matches on one
  crank) but low virality and not mobile-shaped.
- **Mass Raffle / Survival Pool**: easiest to build and legible to the DeFi crowd
  (mass payout to winners), but mass payout is the canonical *pull-based* use case, so
  it under-sells the unique push-mode value. Good as a DeFi-flavored secondary demo.

---

## Recommendation

Lead with **Elimination Royale** as the hero demo: highest combined virality +
showcase-fit + mobile-fit, and the cleanest "this is literally impossible without a
resumable push-mode effect" narrative. Keep **Game of Life** in the back pocket as a
30-second "and here's the same primitive ticking 10,000 cells" tech-flex clip for the
benchmark writeup.

If the goal shifts from "one viral moment" to "a product with retention," switch the
hero to **Critter World**.

> Mobile note: all top picks favor async/turn-based loops + push notifications, which
> map cleanly to Solana Mobile and mobile web. Confirm wallet/UX path (Mobile Wallet
> Adapter) before committing to a native build vs. mobile-first PWA.
