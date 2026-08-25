# Onboarding: RUM (Real User Monitoring)  *(STUB — for the RUM PM to complete)*

> **Status:** scaffold. The orchestrator routes here; the RUM PM completes it from the template
> (`contributing/onboarding-reference-md-template.md`).

Instrument a browser or mobile app so real-user sessions, page loads, web vitals and front-end errors
reach Coralogix RUM. "Onboarded" means: sessions and web-vitals appear in the RUM UI.

## When to use this reference

"Set up RUM", "monitor front-end performance", "track real users / page loads / web vitals",
"capture browser JS errors".

## Prerequisites (in order)

1. **Deploy the RUM integration package** in Coralogix — this creates the **RUM public key** the SDK
   needs. This key is *publicly visible* and is **distinct from the Send-Your-Data API key**.
2. **The RUM SDK** for the platform (NPM browser SDK / CDN snippet, or the mobile / React Native /
   Flutter SDK).
3. **Application name, version, and `coralogixDomain`** (a **region enum** like `EU2`/`US1`, *not* an
   ingress hostname) to configure the SDK.

> **Not the OTLP protobuf/gRPC path.** RUM is a **front-end** signal shipped by the RUM SDK over its own
> transport with the RUM public key — the protobuf/Bearer/`ingress:443` prerequisites from the other
> references do **not** apply here. Don't conflate them.

## Minimal config (happy path) — browser (NPM)

```js
import { CoralogixRum } from '@coralogix/browser';

CoralogixRum.init({
  public_key: '<rum-public-key>',
  application: '<app>',
  version: '1.0.0',
  coralogixDomain: 'EU2',      // region enum for your account (EU1/EU2/US1/US2/AP1/AP2…)
});
```

*TODO (RUM PM):* confirm the current package name/version, recommended optional params
(`user_context`, `labels`, `ignoreUrls`, session sampling), the CDN-snippet variant, and the
mobile/React Native/Flutter equivalents.

## Verify (close the loop)

*TODO (RUM PM):* how to confirm sessions arrived (UI check; RUM data is `$d.cx_rum.*` when queried via
`cx-telemetry-querying`). Example:
```bash
cx logs "filter \$d.cx_rum.application_name == '<app>'" --start now-15m --limit 5
```

## Common failures → fixes / Tier & cost / AI layers / Docs deep-links

*TODO (RUM PM)* — follow the template. Docs deep-links (verified 2026-07-07):
- [RUM SDK installation — overview](https://coralogix.com/docs/user-guides/rum/sdk-installation/overview/)
- [NPM browser SDK installation guide](https://coralogix.com/docs/user-guides/rum/sdk-installation/javascript/npm-browser/)

Prefer these canonical pages over guessing a URL; or use `cx docs search "RUM setup"`.

## Sources / evidence

Scaffold (2026-07). Fill from the RUM docs linked above + real cases.
