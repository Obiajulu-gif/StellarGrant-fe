# StellarGrants Protocol

<div align="center">

**Milestone-based grant management on the Stellar blockchain — on-chain escrow, DAO voting, and contributor reputation, all in one open-source monorepo.**

[![CI](https://github.com/StellarGrant/stellargrant-fe/actions/workflows/ci.yml/badge.svg)](https://github.com/StellarGrant/stellargrant-fe/actions)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![Next.js](https://img.shields.io/badge/Next.js-16-black)](https://nextjs.org/)
[![Stellar SDK](https://img.shields.io/badge/Stellar%20SDK-13.x-7D00FF)](https://stellar.github.io/js-stellar-sdk/)
[![Contributors](https://img.shields.io/github/contributors/StellarGrant/stellargrant-fe)](https://github.com/StellarGrant/stellargrant-fe/graphs/contributors)


[Overview](#overview) • [Features](#features) • [Architecture](#architecture) • [Quick Start](#quick-start) • [Packages](#packages) • [Contributing](#contributing) • [Security](#security)

</div>

---

## Overview

StellarGrants Protocol is a fully decentralized grant-management system built on [Stellar (Soroban)](https://developers.stellar.org/docs/build/smart-contracts/overview). It allows grant creators to post milestone-gated bounties, contributors to submit work with on-chain proof, and a decentralized reviewer committee to vote on approvals — with automatic token payouts from on-chain escrow the moment consensus is reached.

All protocol state lives on the Soroban smart contract. The Next.js frontend reads contract state directly from Stellar RPC — no centralized backend is required for any core feature.

### Who is it for?

| Role | What they do |
|------|--------------|
| **Grant Creator** | Post grants with budget, milestones, reviewer list, and token |
| **Contributor** | Browse open grants, submit milestone work with IPFS proof |
| **Reviewer** | Vote approve / reject on milestone submissions |
| **Funder** | Deposit XLM or USDC into a grant's on-chain escrow |

---

## Features

### Core Protocol

- **Milestone-Based Escrow** — Funds are locked in a Soroban contract and only released when a milestone is approved by the designated reviewer quorum
- **DAO Voting** — Every milestone requires a configurable quorum of reviewer approvals before payout is triggered
- **Automatic Payout** — No admin intervention: the contract executes the token transfer as soon as the vote threshold is reached
- **Dispute Resolution** — Contributors or funders can raise disputes on rejected milestones for arbitration
- **Multi-Token Support** — Grants can be denominated in native XLM or USDC (any SEP-41 token)
- **Contributor Reputation** — On-chain reputation scoring tracks completed milestones and participation history

### Frontend Application

- **Wallet-First UX** — Connect Freighter, xBull, or use Stellar Passkeys (WebAuthn/Secp256r1) in one click
- **Zero-Backend Architecture** — All state reads go directly to Stellar RPC; no custom API needed
- **Real-Time Event Streaming** — Subscribe to contract events via Server-Sent Events for live vote counts and funding progress
- **Multi-Step Grant Creation** — Four-step guided form with Zod validation, milestone builder, and budget configurator
- **IPFS Proof Submission** — Contributors upload milestone evidence to IPFS via Pinata; the CID is stored on-chain
- **Leaderboard & Profiles** — Contributor reputation board and individual profile pages with GitHub handle and skills
- **Responsive & Accessible** — Mobile-first Tailwind UI with ARIA labels, keyboard shortcuts, and dark theme
- **Storybook Component Library** — All UI components are documented and previewed in Storybook

---

## Repository Layout

```
stellargrant-fe/                  ← Monorepo root
├── stellargrant-fe/              ← Next.js 16 frontend (primary package)
├── stellargrant-contracts/       ← Soroban smart contracts (Rust → WASM)
├── client/                       ← @stellargrants/client-sdk (TypeScript SDK)
├── api/                          ← Optional Express + TypeORM caching API
├── docker-compose.yml            ← Postgres + API service
├── .github/workflows/ci.yml      ← GitHub Actions CI
├── TUTORIAL.md                   ← Beginner end-to-end walkthrough
├── CONTRIBUTING.md               ← Root contribution guide (this repo)
├── ARCHITECTURE.md               ← Deep-dive architecture reference
├── DEVELOPMENT.md                ← Full developer environment setup
├── CODE_OF_CONDUCT.md            ← Community standards
└── SECURITY.md                   ← Vulnerability reporting policy
```

| Package | Tech | Purpose |
|---------|------|---------|
| [`stellargrant-fe/`](stellargrant-fe/) | Next.js 16, React 19, TypeScript | Full-featured web UI — grant browsing, creation, funding, milestone voting |
| [`stellargrant-contracts/`](stellargrant-contracts/) | Rust, Soroban SDK | Smart contract: escrow, milestones, voting, payouts, events |
| [`client/`](client/) | TypeScript, stellar-sdk | `@stellargrants/client-sdk` — programmatic contract access for Node or bundlers |
| [`api/`](api/) | Express, TypeORM, PostgreSQL | Optional middleware layer for caching, indexing, and server-side flows |

---

## Architecture

### System Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        Browser (User)                           │
│                                                                 │
│  ┌──────────────────┐    ┌─────────────────┐                   │
│  │  Next.js Frontend│    │ Wallet Extension │                   │
│  │  (React / SSR)   │    │(Freighter/xBull) │                   │
│  └────────┬─────────┘    └────────┬────────┘                   │
│           │ reads contract state   │ signs transactions          │
└───────────┼────────────────────────┼────────────────────────────┘
            │                        │
            ▼                        ▼
┌───────────────────────────────────────────────────────────────┐
│                      Stellar Network                           │
│                                                               │
│  ┌─────────────────────┐    ┌───────────────────────────────┐ │
│  │  Soroban RPC Node   │    │  Horizon API                  │ │
│  │  (simulateTx /      │    │  (account info / balances /   │ │
│  │   sendTx / events)  │    │   trustlines)                 │ │
│  └──────────┬──────────┘    └───────────────────────────────┘ │
│             │                                                  │
│             ▼                                                  │
│  ┌──────────────────────────────────────────────────────────┐ │
│  │            StellarGrants Soroban Contract                │ │
│  │  grant_create · grant_fund · milestone_submit           │ │
│  │  milestone_vote · milestone_payout · dispute_raise      │ │
│  └──────────────────────────────────────────────────────────┘ │
└───────────────────────────────────────────────────────────────┘
            │
            ▼ (optional)
┌───────────────────────┐
│  Express API (api/)   │  ← caching, indexing, SSE relay
│  PostgreSQL           │
└───────────────────────┘
```

### Key Design Decisions

| Decision | Why |
|----------|-----|
| Zero-backend for core flows | All grant data is on-chain; no server can become a single point of failure or censor data |
| Soroban smart contract | Native Stellar programmability — SEP-41 tokens, events, deterministic execution |
| Next.js App Router with Server Components | SEO for grant pages + streaming data from RPC without client waterfalls |
| TanStack Query for client cache | Declarative loading/stale states, background refetch, and optimistic UI without Redux boilerplate |
| Zustand for wallet state | Minimal, persistent wallet session without prop drilling |
| IPFS for milestone proof | Decentralized proof storage; only the content hash (CID) is stored on-chain |

For a full architecture reference including rendering strategy, state management, and data flow diagrams, see [ARCHITECTURE.md](ARCHITECTURE.md).

---

## Quick Start

### Prerequisites

| Tool | Version | Notes |
|------|---------|-------|
| Node.js | >= 20 | Use [nvm](https://github.com/nvm-sh/nvm) to manage versions |
| npm | >= 10 | Lockfiles committed per package |
| Rust | stable | Required for smart contracts only |
| `wasm32-unknown-unknown` target | — | `rustup target add wasm32-unknown-unknown` |
| [Stellar CLI](https://developers.stellar.org/docs/tools/developer-tools) | latest | Required for contract deploy/invoke |
| [Freighter Wallet](https://freighter.app) | latest | Browser extension for testing wallet flows |

---

### 1 — Clone the Repository

```bash
git clone https://github.com/StellarGrant/stellargrant-fe.git
cd stellargrant-fe
```

---

### 2 — Frontend (Primary)

```bash
cd stellargrant-fe
npm ci

# Copy environment template and fill in your values
cp .env.local.example .env.local
# See Configuration section below for required variables

# Start development server (Turbopack)
npm run dev
```

Open [http://localhost:3000](http://localhost:3000). The app reads from Stellar testnet by default.

To run the frontend alongside a mock API (for offline development):

```bash
npm run dev:mock      # starts both mock server (port 4000) and Next.js (port 3000)
```

---

### 3 — Smart Contracts

```bash
cd stellargrant-contracts

# Add WASM target if not already added
rustup target add wasm32-unknown-unknown

# Format check, lint, compile check (mirrors CI)
cargo fmt --all -- --check
cargo clippy --workspace --lib --target wasm32-unknown-unknown -- -D warnings
cargo check --workspace --target wasm32-unknown-unknown

# Run contract unit tests
cargo test

# Build WASM binary
cd contracts/stellar-grants
make build
```

#### Deploy to Testnet

```bash
cd stellargrant-contracts/contracts/stellar-grants
make build

stellar contract deploy \
  --wasm target/wasm32v1-none/release/stellar_grants.wasm \
  --network testnet \
  --source-account YOUR_ACCOUNT_ALIAS \
  --alias stellar_grants

# Initialize contract state
stellar contract invoke \
  --id stellar_grants \
  --network testnet \
  --source-account YOUR_ACCOUNT_ALIAS \
  -- initialize
```

Copy the deployed contract address into `NEXT_PUBLIC_CONTRACT_ID` in your `.env.local`.

---

### 4 — TypeScript Client SDK

```bash
cd client
npm ci
npm run build
npm test
```

---

### 5 — Optional Express API

```bash
cd api
npm ci

# Set up environment (requires PostgreSQL)
cp .env.example .env
# Edit DATABASE_URL in .env

npm run dev          # starts on port 4000
```

Or run the full stack with Docker:

```bash
docker compose up    # Postgres + API
```

---

## Configuration

### Frontend (`stellargrant-fe/.env.local`)

Create this file from the template — **never commit it**.

```env
# ── Stellar Network ─────────────────────────────────────────────
NEXT_PUBLIC_STELLAR_NETWORK=testnet
# Options: testnet | mainnet | futurenet

NEXT_PUBLIC_STELLAR_RPC_URL=https://soroban-testnet.stellar.org
# Mainnet: https://soroban.stellar.org

NEXT_PUBLIC_NETWORK_PASSPHRASE=Test SDF Network ; September 2015
# Mainnet: Public Global Stellar Network ; September 2015

NEXT_PUBLIC_HORIZON_URL=https://horizon-testnet.stellar.org
# Mainnet: https://horizon.stellar.org

# ── Contract ────────────────────────────────────────────────────
NEXT_PUBLIC_CONTRACT_ID=CXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
# Your deployed StellarGrants contract address

# ── Token Addresses ─────────────────────────────────────────────
NEXT_PUBLIC_NATIVE_TOKEN=CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC
NEXT_PUBLIC_USDC_TOKEN=CBIELTK6YBZJU5UP2WWQEUCYKLPU6AUNZ2BQ4WWFEIE3USCIHMXQDAMA

# ── IPFS (Pinata) — for milestone proof uploads ─────────────────
NEXT_PUBLIC_IPFS_GATEWAY=https://gateway.pinata.cloud/ipfs
PINATA_API_KEY=your_pinata_api_key          # server-only
PINATA_SECRET_KEY=your_pinata_secret_key    # server-only

# ── Optional API Backend ─────────────────────────────────────────
NEXT_PUBLIC_API_URL=http://localhost:4000

# ── Analytics (optional) ─────────────────────────────────────────
NEXT_PUBLIC_POSTHOG_KEY=
NEXT_PUBLIC_POSTHOG_HOST=https://app.posthog.com
```

> **Security**: Variables prefixed `NEXT_PUBLIC_` are bundled into the client. Never prefix secrets (API keys, private keys) with `NEXT_PUBLIC_`.

### API (`api/.env`)

```env
PORT=4000
DATABASE_URL=postgres://user:password@localhost:5432/stellargrant
```

---

## Packages

### `stellargrant-fe/` — Next.js Frontend

The primary user-facing application. Key sub-directories:

```
app/                 Next.js App Router — pages and API routes
components/          React components (UI primitives + domain-specific)
  ui/                shadcn/ui base components (Button, Card, Dialog …)
  grants/            Grant cards, creation form, funding progress
  milestones/        Milestone list, proof submission, vote panel
  wallet/            Wallet connect modal, wallet guard
  leaderboard/       Contributor reputation table
  dispute/           Dispute submission and status
  layout/            Header, footer, sidebar, notification bell
hooks/               TanStack Query + Zustand powered custom hooks
lib/
  stellar/           Stellar SDK wrappers (RPC client, contract calls, event streaming)
  store/             Zustand stores (wallet session)
  wallets/           Adapter pattern — Freighter, Albedo, xBull
  schemas/           Zod validation schemas for forms and API responses
  ipfs/              Pinata upload helpers
types/               Shared TypeScript interfaces (Grant, Milestone, Contributor …)
tests/               Vitest unit and component tests
e2e/                 Playwright end-to-end tests
stories/             Storybook component stories
```

**Available Scripts**

| Command | Description |
|---------|-------------|
| `npm run dev` | Dev server with Turbopack |
| `npm run dev:mock` | Dev server + mock API concurrently |
| `npm run build` | Production build |
| `npm start` | Start production server |
| `npm run lint` | ESLint |
| `npm test` | Vitest (watch mode) |
| `npm run test:run` | Vitest (single run) |
| `npm run test:e2e` | Playwright E2E tests |
| `npm run mock` | Start mock API only |
| `npm run storybook` | Storybook on port 6006 |
| `npm run build-storybook` | Build static Storybook |

### `stellargrant-contracts/` — Soroban Contracts

Rust contracts compiled to WASM and deployed to Stellar. Core contract entry points:

| Function | Description |
|----------|-------------|
| `grant_create` | Create a new grant with owner, title, budget, milestones, reviewer list |
| `grant_fund` | Deposit tokens into the grant's escrow |
| `milestone_submit` | Submit proof of work for a milestone (IPFS CID + notes) |
| `milestone_vote` | Reviewer casts approve/reject vote; triggers payout when quorum reached |
| `dispute_raise` | Open a dispute on a rejected milestone |
| `contributor_register` | Register a contributor's GitHub handle and skills |

### `client/` — TypeScript SDK

`@stellargrants/client-sdk` provides a typed interface to the Soroban contract from Node.js or any bundler. Useful for scripts, bots, and integration tests.

```bash
cd client && npm ci && npm run build
```

### `api/` — Express Caching API

Optional Express + TypeORM service that indexes on-chain events into PostgreSQL for faster queries and server-side validation. Not required for core frontend functionality.

---

## Pages & Routes

| Route | Description |
|-------|-------------|
| `/` | Landing page — protocol stats, featured grants, call to action |
| `/grants` | Paginated, filterable grant listing with status / token / sort filters |
| `/grants/[id]` | Grant detail — funding progress, milestone timeline, reviewer panel, event history |
| `/grants/create` | Multi-step grant creation form (wallet required) |
| `/grants/[id]/fund` | Fund a grant — deposit XLM or USDC into escrow |
| `/grants/[id]/milestones` | Milestone overview for a grant |
| `/grants/[id]/milestones/[idx]` | Single milestone — proof viewer, vote panel, payout status |
| `/dashboard` | User dashboard — my grants, activity feed, pending actions |
| `/profile` | Connected wallet's profile — skills, reputation, grant history |
| `/contributors/[address]` | Public contributor profile page |
| `/leaderboard` | Global contributor reputation ranking |
| `/review` | Reviewer queue — pending milestones awaiting your vote |
| `/dispute` | Dispute management interface |
| `/search` | Full-text grant search |
| `/settings` | User preferences |

---

## Testing

### Unit & Component Tests (Vitest)

```bash
cd stellargrant-fe
npm test             # watch mode
npm run test:run     # single pass with coverage
```

Tests live in `tests/` and co-located `*.test.tsx` files.

### End-to-End Tests (Playwright)

```bash
cd stellargrant-fe
npm run test:e2e             # headless
npm run test:e2e:headed      # with browser visible
```

E2E tests cover critical user flows: grant creation, funding, milestone submission, and reviewer voting.

### Contract Tests (Rust)

```bash
cd stellargrant-contracts
cargo test
```

### API Tests

```bash
cd api
npm run test:e2e           # end-to-end API tests
npm run test:integration   # integration tests
```

---

## CI / CD

GitHub Actions workflow: [`.github/workflows/ci.yml`](.github/workflows/ci.yml)

Runs on every pull request and push to `main`:

| Job | Steps |
|-----|-------|
| **contracts** | `cargo fmt` check, `cargo clippy` (WASM, deny warnings), `cargo check` |
| **frontend** | `npm ci`, `npm run lint`, `npm run build` |
| **api** | `npm ci`, migrations, integration + E2E tests |
| **client-sdk** | `npm ci`, `npm run build`, Jest tests |

Always run `npm run lint`, `npm run test:run`, and `npm run build` locally before opening a PR.

---

## Deployment

### Vercel (Recommended for Frontend)

```bash
npm install -g vercel
cd stellargrant-fe
vercel           # preview
vercel --prod    # production
```

Set all `NEXT_PUBLIC_*` variables and server-side secrets in Vercel's Environment Variables dashboard. Use separate testnet values for Preview and mainnet values for Production.

### Docker (API + Postgres)

```bash
docker compose up --build
```

The `api/Dockerfile` builds a Node 20-alpine image. PostgreSQL is provisioned as a compose service.

---

## Wave Program

The StellarGrants Protocol participates in the **Stellar Wave Program** on [Drips](https://drips.network/wave/stellar). Frontend and contract issues labeled `drips-wave` are eligible for Wave Point rewards.

**Tips for Wave contributors:**
- Comment on an issue to claim it before starting work
- Open a draft PR early to get feedback
- Include before/after screenshots for all UI changes
- Complete the full PR checklist before requesting review

---

## Contributing

We welcome contributions of all kinds — bug fixes, new features, documentation, tests, and contract improvements. With over 60 contributors, StellarGrants is an active, community-driven project.

- **Root contribution guide**: [CONTRIBUTING.md](CONTRIBUTING.md)
- **Frontend-specific guide**: [stellargrant-fe/CONTRIBUTING.md](stellargrant-fe/CONTRIBUTING.md)
- **Contract contribution guide**: [stellargrant-contracts/ContributionGuide.md](stellargrant-contracts/ContributionGuide.md)
- **Architecture reference**: [ARCHITECTURE.md](ARCHITECTURE.md)
- **Developer setup**: [DEVELOPMENT.md](DEVELOPMENT.md)
- **Code of Conduct**: [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)
- **Beginner tutorial**: [TUTORIAL.md](TUTORIAL.md)

---

## Security

- Run all tests and linters locally before pushing to public networks
- Review access control and numeric safety for every contract change
- Never commit private keys, seeds, or production secrets to this repository
- Report vulnerabilities via [GitHub Security Advisories](https://github.com/StellarGrant/stellargrant-fe/security/advisories) — see [SECURITY.md](SECURITY.md) for the full policy

---

## License

This project is licensed under the [MIT License](LICENSE).

---

<div align="center">

**Fix. Merge. Earn.** | [Stellar Wave Program](https://drips.network/wave/stellar)

Made with care for the Stellar ecosystem by [60+ open-source contributors](https://github.com/StellarGrant/stellargrant-fe/graphs/contributors)

</div>
