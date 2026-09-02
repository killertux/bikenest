# BikeNest — Bicycle Parking Finder

## Requirements & Technical Specification

---

# 1. Product overview

Build a web application that helps cyclists find suitable places to park their bicycles near a destination.

The primary user flow is:

> **Enter an address or place → find nearby bicycle parking → filter and compare options → view details → navigate to the selected location.**

The application should provide information that helps a cyclist decide **where they would actually feel comfortable leaving their bicycle**, including:

* distance;
* cost;
* security;
* parking type;
* opening hours;
* photos;
* community ratings;
* information freshness;
* verification confidence.

The application should eventually become a community-maintained database of bicycle parking locations.

---

# 2. Scope

The initial product includes all currently planned functionality.

## Included

* Web application
* Address/place search
* Current-location search
* Interactive map
* Nearby bicycle parking discovery
* Parking details
* Cost information
* Security information
* Opening hours
* Photos
* External navigation
* Search filtering
* Search sorting/recommendation
* User accounts
* Email/password authentication
* Email verification
* Google authentication
* Extensible authentication-provider architecture
* Add parking locations
* Edit/propose corrections
* Reviews
* Verification
* Reporting
* Favorites
* Contribution history
* "I parked here" verification
* Basic moderation
* Moderation dashboard (photo/contribution review queue)
* Privacy/data-subject functionality
* Account deletion/anonymization
* Personal-data export

## Explicitly out of scope

The initial version MUST NOT include:

* Native iOS application
* Native Android application
* Mobile-specific application architecture
* In-app navigation
* Parking reservations
* Payments
* Smart-lock integration
* Real-time parking availability
* Commercial parking operator integrations
* Social following/friends
* Private messaging between users

The web application MUST nevertheless be responsive and usable from mobile browsers.

---

# 3. Architecture principles

The application MUST follow **Clean Architecture principles**.

The architecture should:

* separate business/domain logic from infrastructure;
* keep framework-specific concerns at the boundaries;
* keep database implementation details out of domain logic;
* keep external service providers behind abstractions;
* make business logic independently testable;
* make infrastructure replaceable where practical;
* avoid coupling domain concepts to HTTP, HTMX, PostgreSQL, OAuth providers, or map providers.

Dependencies should generally point **inward toward the domain/application layers**, not outward toward infrastructure.

The implementation agent MUST explicitly document the architectural boundaries in its implementation plan.

The implementation plan MUST include a dependency graph demonstrating that the dependency direction is respected.

---

# 4. Rust workspace and crate architecture

The application MUST be organized as a **Cargo workspace containing multiple crates**, with crate boundaries reflecting the Clean Architecture.

The exact crate names and decomposition should be determined by the implementation plan, but the architecture SHOULD resemble:

```text
workspace/
├── crates/
│   ├── domain/
│   ├── application/
│   ├── infrastructure/
│   ├── web/
│   └── test-support/
│
├── migrations/
├── Cargo.toml
└── ...
```

A more detailed decomposition MAY be appropriate, for example:

```text
domain
application
persistence
authentication
geocoding
storage
web
test-support
```

The agent MUST avoid creating crates merely for the sake of having many crates. Each crate should have a clear architectural responsibility.

## Domain crate

Contains concepts such as:

* User
* AuthenticationIdentity
* ParkingLocation
* ParkingSecurityFeature
* ParkingReview
* ParkingVerification
* ParkingContribution
* Favorite
* ParkingReport
* ModerationState
* AuditEvent

It MUST NOT depend on:

* Axum
* SQLx
* Askama
* HTMX
* PostgreSQL-specific infrastructure
* external API clients

---

# 5. Application layer

The application layer SHOULD contain the use cases of the system.

Examples:

```text
SearchParking
GetParkingDetails
CreateParkingLocation
UpdateParkingLocation
ProposeParkingChange
ReviewParking
VerifyParking
ReportParking
FavoriteParking
AddParkingPhoto
RegisterUser
VerifyEmail
AuthenticateUser
AuthenticateWithGoogle
ChangePassword
RequestPasswordReset
ResetPassword
ChangeEmail
ExportUserData
RequestAccountDeletion
ProcessPrivacyRequest
ModerateReport
ModerateContribution
```

Application services SHOULD coordinate domain objects and ports/interfaces.

The application layer MUST NOT directly depend on Axum handlers or HTML templates.

---

# 6. Infrastructure layer

Infrastructure SHOULD implement interfaces required by the application/domain.

Examples:

* PostgreSQL repositories
* SQLx queries
* Google OAuth
* password hashing
* email delivery
* image storage
* geocoding provider
* external map services
* rate-limit storage
* application clock
* random/token generation

Infrastructure-specific implementations MUST remain outside the domain layer.

---

# 7. Web layer

The web layer is responsible for:

* HTTP routing;
* request parsing;
* authentication/session extraction;
* authorization at the HTTP boundary;
* invoking application use cases;
* rendering Askama templates;
* returning HTMX responses;
* serving static assets;
* HTTP security headers;
* CSRF handling;
* HTTP-level rate limiting.

The web layer MUST NOT contain business rules that belong in the application/domain layers.

---

# 8. Database

The database MUST be:

**PostgreSQL**

The application SHOULD use:

**PostGIS**

for geographic data and proximity queries.

The database schema MUST support at minimum:

* users;
* authentication identities;
* sessions;
* email verification tokens;
* password reset tokens;
* parking locations;
* security features;
* opening hours;
* photos;
* reviews;
* verifications;
* contributions;
* reports;
* favorites;
* moderation actions;
* audit events;
* privacy requests.

The implementation plan MUST document:

* primary-key strategy;
* timestamp strategy;
* timezone handling;
* foreign-key strategy;
* deletion behavior;
* indexing strategy;
* uniqueness constraints;
* PostGIS indexes;
* transaction boundaries.

Event timestamps (creation, update, verification, review, and similar point-in-time values) MUST be stored consistently as UTC timestamps.

User-facing event timestamps SHOULD be rendered in the viewer's timezone.

Opening hours are NOT event timestamps and MUST NOT be stored as UTC — see section 29.

---

# 9. SQLx

The backend MUST use **SQLx** for database access.

The application MUST use **SQL queries written directly by the developers**.

It MUST NOT use an ORM or ORM-style abstraction.

For example:

```rust
sqlx::query!(
    r#"
        SELECT id, name, latitude, longitude
        FROM parking_location
        WHERE ...
    "#
)
```

rather than:

```text
repository.find(...)
```

where the repository internally constructs ORM queries.

Repositories are still encouraged as **architectural boundaries**, but their implementations should use explicit SQL.

## SQL requirements

Queries SHOULD:

* be explicit;
* be easy to inspect;
* avoid dynamically generated SQL unless genuinely necessary;
* use parameterized values;
* keep database-specific behavior inside the persistence/infrastructure layer.

PostGIS queries MUST be expressed directly in SQL.

SQLx compile-time query checking SHOULD be used where practical.

---

# 10. Database migrations

Database schema changes MUST be managed through migrations.

Migrations MUST be version-controlled.

The implementation plan MUST specify:

* migration tooling;
* migration directory structure;
* local development workflow;
* test database migration workflow;
* production migration workflow;
* migration rollback strategy where practical.

The application MUST NOT rely on automatically generated database schemas at runtime.

Production migrations MUST be explicit and reproducible.

---

# 11. Dependency management

Whenever a new Rust dependency is required, the implementation MUST use:

```bash
cargo add <dependency>
```

The developer MUST NOT manually add dependency versions to `Cargo.toml` as the normal workflow.

The default behavior should therefore be to use the latest compatible version available from crates.io.

If a dependency requires feature flags, those SHOULD also be configured through `cargo add`.

Example:

```bash
cargo add sqlx --features runtime-tokio-rustls,postgres,uuid,chrono,migrate
```

The implementation agent MUST verify the appropriate current feature set rather than blindly copying an old example.

Dependencies SHOULD only be introduced when they provide meaningful value.

---

# 12. Frontend architecture

The application MUST be a **server-rendered web application**.

The frontend MUST use:

* HTML
* HTMX 4
* Askama
* Tailwind CSS 4.3
* Alpine.js where client-side state is genuinely necessary

The application MUST use **Tailwind CSS 4.3** as its styling system:

* design tokens (colors, typography, spacing, radii, shadows) MUST be defined as Tailwind theme tokens (`@theme` in CSS) rather than scattered custom CSS;
* the tokens from the exported design (`design-project/colors_and_type.css`, see `UI_DESIGN.md`) MUST be mapped into the Tailwind theme so the implemented UI matches the approved design;
* Tailwind's built-in build/CLI tooling MUST be used — no CDN/runtime-JIT in production;
* custom CSS SHOULD be limited to things Tailwind cannot express well (e.g. MapLibre overrides) and SHOULD be kept in one well-known stylesheet loaded after the Tailwind entry.

The application SHOULD minimize custom JavaScript.

The server SHOULD remain the primary source of truth for business and server state.

---

# 13. HTMX

HTMX MUST be used for dynamic server-driven interactions.

Examples:

* search autocomplete;
* parking search;
* filtering;
* sorting;
* loading parking details;
* login/register interactions;
* adding/editing parking;
* submitting reviews;
* verification;
* favorites;
* reports;
* moderation actions;
* privacy/account operations.

HTMX requests SHOULD generally receive HTML fragments rendered by Askama rather than JSON.

The server should remain the primary source of truth for UI state.

---

# 14. Alpine.js

**Alpine.js MUST be available for client-side state management where necessary.**

It SHOULD be used when state is primarily local to the browser/UI and would be unnecessarily cumbersome to implement with HTMX alone.

Appropriate examples include:

* map UI state;
* opening/closing complex UI elements;
* filter panel state;
* dropdowns;
* image galleries;
* modal state;
* temporary UI state;
* client-side interaction around map markers;
* browser geolocation state.

Alpine.js SHOULD NOT become a general frontend application framework.

The application SHOULD follow:

```text
Server state/business logic
        ↓
      HTMX

Local UI state
        ↓
    Alpine.js
```

rather than building a large client-side state-management architecture.

---

# 15. Askama

Askama MUST be used for server-side HTML rendering.

Templates SHOULD be organized into reusable layouts, components and HTMX partials.

A possible structure:

```text
templates/
├── layouts/
│   └── base.html
├── pages/
│   ├── home.html
│   ├── parking.html
│   ├── login.html
│   ├── register.html
│   ├── account.html
│   └── privacy.html
├── components/
│   ├── parking_card.html
│   ├── parking_rating.html
│   ├── parking_security.html
│   ├── photo_gallery.html
│   └── ...
└── partials/
    ├── search_results.html
    ├── parking_results.html
    ├── parking_details.html
    ├── filters.html
    └── ...
```

The implementation may use a different structure if it better fits the final crate architecture.

---

# 16. Authentication

The application MUST support:

## Email/password

Users can:

* register;
* verify their email;
* log in;
* log out;
* change password;
* request password reset;
* reset their password;
* change their email address.

Passwords MUST be securely hashed using a modern password hashing algorithm appropriate for password storage.

Passwords MUST never be stored in plaintext.

## Email verification

New email/password accounts MUST require email verification.

Verification tokens MUST:

* be cryptographically random;
* expire;
* be single-use;
* be invalidated after successful use;
* not be stored in plaintext where avoidable.

The implementation plan MUST define what authenticated functionality is available before email verification.

The default should be:

```text
Unverified account:
- may complete account setup;
- may browse/search;
- may not create public contributions;
- may not review;
- may not verify parking;
- may not upload photos.
```

## Google

Users MUST be able to authenticate using Google OAuth.

Google authentication MUST map to the same internal user model as email/password authentication.

---

# 17. Extensible authentication

Authentication MUST be designed to support additional providers in the future.

The system SHOULD distinguish:

```text
User
```

from:

```text
AuthenticationIdentity
```

Conceptually:

```text
User
 ├── Email/password identity
 ├── Google identity
 └── Future identity
```

The implementation SHOULD make adding another provider primarily an infrastructure/application concern rather than requiring changes throughout the domain.

Authentication identities MUST NOT be publicly exposed.

---

# 18. Sessions

The application MUST use secure server-side authentication sessions.

Authentication cookies MUST use appropriate:

* `HttpOnly`;
* `Secure`;
* `SameSite`

settings.

The default production configuration SHOULD use:

```text
HttpOnly = true
Secure = true
SameSite = Lax
```

unless a specific architectural requirement justifies another configuration.

The system MUST protect authenticated operations against CSRF.

Sessions MUST support explicit invalidation.

Password changes and account-security events SHOULD invalidate existing sessions where appropriate.

Session identifiers MUST be unpredictable and MUST NOT contain user information.

---

# 19. User roles

The initial system MUST support at least:

```text
USER
MODERATOR
ADMIN
```

Authorization MUST be enforced in the application layer rather than relying exclusively on UI visibility.

The implementation SHOULD use deny-by-default authorization.

Role assignment MUST be an explicit, audited operation:

* The initial ADMIN account is seeded (for example via an idempotent CLI command or migration driven by an environment-provided credential), never created through an unauthenticated public endpoint.
* ADMIN users MAY grant and revoke the MODERATOR and ADMIN roles on other accounts.
* Role changes MUST be denied by default, MUST require an ADMIN principal, and MUST create an audit event (see section 47).
* Role changes MUST NOT be possible through self-service account settings.

---

# 20. Account lifecycle

User accounts MUST have an explicit lifecycle.

At minimum:

```text
PENDING_EMAIL_VERIFICATION
        ↓
ACTIVE
        ↓
SUSPENDED
        ↓
DELETED / ANONYMIZED
```

The implementation plan MUST define what each state can do.

Suspended users MUST not be able to perform normal contribution actions.

Deleted accounts MUST not retain publicly visible personally identifying information.

---

# 21. Functional requirements

## Search

Users MUST be able to search by:

* address;
* business;
* landmark;
* neighborhood;
* city;
* current location.

Search results MUST be geocoded into coordinates.

Search requests MUST be rate limited to prevent abuse.

---

# 22. Browser geolocation

The application MAY request browser geolocation.

Geolocation MUST:

* only be requested after a clear user action;
* use the browser's permission mechanism;
* not be persisted by default;
* not be associated with the user's account merely because the user performed a search;
* only be sent to external services when required for the requested operation.

The default behavior is:

```text
Browser location
      ↓
Search request
      ↓
Find nearby parking
      ↓
Discard location
```

rather than:

```text
Browser location
      ↓
Store permanently in user history
```

Any persistent geolocation feature would require an explicit future product/privacy decision.

---

# 23. Map

The application MUST display:

* destination;
* bicycle parking markers;
* current position where available.

The user MUST be able to:

* pan;
* zoom;
* select markers;
* select result cards;
* return to destination/current position.

Map rendering SHOULD use MapLibre GL JS.

The map tile/style provider MUST be replaceable.

The application MUST provide a non-map representation of search results.

The map MUST NOT be the only way to access parking information.

---

# 24. Parking locations

Each location MUST support:

* name;
* coordinates;
* timezone (IANA identifier, derived from coordinates; see section 29);
* address;
* description;
* parking type;
* cost;
* pricing information;
* opening hours;
* security features;
* photos;
* rating;
* verification information;
* moderation state;
* creation timestamp;
* last meaningful update timestamp;
* last verification timestamp.

---

# 25. Parking lifecycle

Parking locations MUST have an explicit state.

The default states SHOULD be:

```text
ACTIVE
PENDING_REVIEW
FLAGGED
INVALID
REMOVED
```

Normal public search SHOULD only return `ACTIVE` locations.

Moderators/admins MAY inspect other states.

The implementation plan MUST define transitions between states.

---

# 26. Parking types

Initial types:

* Bike rack
* Bicycle parking
* Indoor bicycle parking
* Secured bicycle parking
* Bicycle locker
* Other

The model MUST be extensible.

---

# 27. Cost

Support:

* Free;
* Paid;
* Unknown.

Paid parking SHOULD include:

* amount;
* currency;
* unit.

The currency MUST use an ISO-compatible currency representation.

Examples:

```text
BRL
EUR
USD
```

The system MUST distinguish:

```text
Free
Paid
Unknown
```

from:

```text
Price not currently known
```

---

# 28. Security

Security MUST be represented through individual attributes.

Initial attributes:

* Dedicated locking point
* Indoor
* CCTV
* Staffed
* Security guard
* Controlled access
* Well lit
* Restricted access

The model MUST be extensible.

> **Resolved (implementation):** the catalog of security-attribute **codes** is a hardcoded list in
> the domain (`SECURITY_FEATURE_CODES`), and human-readable **labels are resolved in the presentation
> layer via i18n** — not stored in a database catalog table. This keeps labels localizable (§102) and
> makes adding an attribute a code + translation rather than a schema migration, while remaining
> extensible. Per-location values still live in the `parking_security` table as (code, tri-state).

Security attributes SHOULD support an `unknown` state rather than treating missing information as false.

---

# 29. Opening hours

Parking locations MUST support weekly opening hours.

The model SHOULD support:

* multiple periods per day;
* closed days;
* unknown hours;
* 24-hour operation.

Opening hours MUST be associated with the parking location's relevant timezone (the timezone of the physical location, NOT the timezone of the contributor's browser).

Opening hours MUST be stored as wall-clock time ranges together with the location's IANA timezone identifier. They MUST NOT be converted to UTC: a wall-clock schedule ("opens at 07:00 every day") is fixed in local time and shifts relative to UTC across DST transitions.

The location's timezone SHOULD be derived from its coordinates (latitude/longitude → IANA timezone) and MAY be confirmable/overridable by the contributor. The "currently open" computation MUST convert the current UTC instant into the location's timezone and compare it against the stored wall-clock ranges.

Opening hours SHOULD be displayed in the location's timezone (with that timezone made visible to the viewer), not silently shifted into the viewer's timezone.

The implementation should avoid storing opening hours as opaque free-form text when structured data is possible.

---

# 30. Photos

Parking locations MUST support multiple photos.

Authenticated users SHOULD be able to upload photos.

Image storage MUST be abstracted behind an application/infrastructure boundary.

Uploaded images MUST:

* have a maximum file size;
* have a maximum pixel dimension;
* be validated by actual file content, not merely filename extension;
* use an allowlist of supported image formats;
* be safely decoded;
* be re-encoded where appropriate;
* have EXIF metadata removed;
* not expose original metadata unnecessarily;
* have generated display/thumbnail versions;
* be subject to moderation.

The implementation SHOULD support:

```text
Upload
 ↓
Validate
 ↓
Process
 ↓
Strip metadata
 ↓
Generate derivatives
 ↓
Store
 ↓
Moderate
 ↓
Publish
```

Photos MUST have an explicit moderation lifecycle:

```text
PENDING_REVIEW
      ↓
APPROVED
      ↓
REJECTED
```

Newly uploaded photos start in `PENDING_REVIEW` and are NOT publicly visible until `APPROVED`. Moderators/administrators review the moderation queue (see section 2 "Moderation dashboard") to approve or reject photos. Approved photos MAY later be hidden again via moderation (see section 44).

The original upload SHOULD NOT be publicly accessible by default.

The implementation plan MUST define maximum upload size and supported image formats.

Recommended initial defaults:

```text
Maximum upload size: 10 MB
Maximum dimensions: 20 megapixels
Supported formats: JPEG, PNG, WebP
```

---

# 31. Nearby search

The backend MUST support geographic proximity queries.

Default search radius SHOULD be approximately 1 km.

Supported radius filters SHOULD include:

* 250 m;
* 500 m;
* 1 km;
* 2 km.

The system SHOULD support a configurable maximum radius.

Distance calculations MUST be performed using PostgreSQL/PostGIS.

The database MUST use appropriate spatial indexes.

---

# 32. Search pagination

Search results MUST be paginated.

The implementation SHOULD use cursor/keyset pagination where practical rather than large offset-based pagination.

The initial default SHOULD be:

```text
20 results per page
```

with a configurable maximum of:

```text
100 results per request
```

The implementation MUST prevent unbounded result sets.

Map marker rendering SHOULD be limited to a reasonable number of visible results.

The implementation plan MUST define behavior for large result sets.

---

# 33. Filtering

Users MUST be able to filter by:

* cost;
* parking type;
* security features;
* maximum distance;
* opening status where useful.

Filtering SHOULD update the results using HTMX without requiring a full page navigation.

Filters MUST be represented in URLs where practical so that searches can be bookmarked/shared.

---

# 34. Sorting

Support:

* Recommended;
* Distance;
* Security;
* Rating;
* Recently verified.

The recommendation algorithm SHOULD consider:

```text
distance
security
rating
freshness
verification confidence
```

The weights MUST be configurable in application code rather than embedded throughout the UI/database.

The recommendation algorithm MUST be deterministic for the same input data.

Missing information MUST NOT automatically be treated as the worst possible value unless explicitly defined.

The implementation plan MUST define default behavior for:

* no reviews;
* unknown security;
* unknown cost;
* stale information;
* unverified locations;
* tied scores.

---

# 35. Community functionality

## Add parking

Authenticated, verified users MUST be able to create parking locations.

The system MUST automatically capture:

* creator;
* coordinates;
* creation timestamp.

The creator's identity MUST NOT automatically become public unless explicitly intended by the product.

---

# 36. Duplicate detection

Before creating a parking location, the system SHOULD search for existing nearby locations and warn about likely duplicates.

Duplicate detection SHOULD consider:

* geographic distance;
* name similarity;
* address similarity.

Duplicate detection is advisory and MUST NOT automatically delete or merge records.

---

# 37. Propose changes

Users MUST be able to propose changes to existing parking information.

Examples:

* price;
* opening hours;
* security;
* type;
* description;
* location;
* existence;
* name;
* address.

Important changes SHOULD retain history rather than silently overwriting historical information.

The system SHOULD represent changes as contributions/proposals where appropriate.

Moderators MAY approve, reject or modify proposals.

---

# 38. Reviews

Authenticated, verified users MUST be able to:

* rate;
* review;
* optionally upload photos.

Reviews use a five-star rating.

A user SHOULD normally be limited to one active review per parking location, with the ability to edit it.

Review edits MUST preserve appropriate audit/history information.

Reviews MUST support moderation states.

---

# 39. Verification

Authenticated users MUST be able to verify information.

Examples:

```text
Still exists
No longer exists
Information changed
```

Individual attributes SHOULD also be verifiable.

Each verification MUST record:

* user;
* timestamp;
* parking location;
* attribute;
* result.

The user's identity MUST NOT be publicly exposed merely because they submitted a verification.

The implementation SHOULD prevent obvious automated abuse of verification signals.

---

# 40. Information freshness

The application MUST expose when information was last verified.

Old information SHOULD be visually identified as potentially stale.

Freshness SHOULD contribute to recommendation scoring.

Recommended default freshness categories:

```text
Fresh
Recently verified
Aging
Stale
Very stale
```

The exact time thresholds MUST be configurable.

The initial recommended default is:

```text
Fresh:          < 30 days
Recently:       30–90 days
Aging:          90–180 days
Stale:          180–365 days
Very stale:     > 365 days
```

These thresholds are product defaults, not claims about factual validity.

---

# 41. "I parked here"

Authenticated users SHOULD be able to indicate that they parked at a location.

This is a verification signal.

The system MUST NOT publicly expose individual users' parking histories.

Individual parking events SHOULD have a short retention period and MUST NOT be treated as permanent public activity.

The implementation plan MUST define the retention policy.

The recommended default is:

```text
Private parking event:
retained for 90 days
```

unless a different legal/product requirement applies.

---

# 42. Favorites

Authenticated users MUST be able to favorite/unfavorite locations.

Favorites MUST be associated with the user and parking location.

Favorites MUST NOT be publicly visible.

---

# 43. Reports

Users MUST be able to report:

* nonexistent parking;
* incorrect location;
* incorrect price;
* incorrect hours;
* incorrect security information;
* duplicate;
* inappropriate photo;
* inappropriate review;
* spam;
* abuse;
* other.

Reports MUST have explicit states:

```text
OPEN
UNDER_REVIEW
RESOLVED
DISMISSED
```

Users MUST NOT be able to manipulate or resolve their own reports.

---

# 44. Moderation

Moderators SHOULD be able to:

* review reports;
* hide inappropriate reviews;
* hide inappropriate photos;
* invalidate parking locations;
* review proposed changes;
* inspect contribution history;
* suspend abusive users where authorized;
* restore content where appropriate.

Moderation actions MUST create audit events.

Moderators MUST NOT be able to silently alter audit history.

---

# 45. Abuse prevention and rate limiting

Because the application contains user-generated content, the system MUST implement basic abuse protection.

At minimum:

* authentication endpoint rate limiting;
* password reset rate limiting;
* email verification rate limiting;
* review creation rate limiting;
* report creation rate limiting;
* parking creation rate limiting;
* photo upload rate limiting;
* verification rate limiting.

Rate limits SHOULD be enforced server-side.

The implementation plan MUST choose appropriate defaults and identify which limits should be per:

* IP;
* account;
* authenticated user;
* resource.

Rate limiting SHOULD not expose sensitive information about whether another account exists.

The implementation SHOULD include basic protections against automated account creation and spam.

---

# 46. Contribution history

The application MUST maintain a contribution history sufficient to:

* attribute changes internally;
* support moderation;
* investigate abuse;
* reconstruct important changes;
* support community trust.

Contribution history MUST distinguish between:

* creation;
* update proposal;
* approval;
* rejection;
* verification;
* report;
* moderation.

Public attribution SHOULD use anonymized/limited information rather than exposing email addresses or OAuth identities.

---

# 47. Audit events

Important security, moderation and account actions MUST create audit events.

Audit events SHOULD contain:

```text
actor
action
target
timestamp
result
metadata
```

Examples:

* login security events;
* password changes;
* email changes;
* account deletion;
* moderation actions;
* parking invalidation;
* contribution approval;
* privacy requests.

Audit records MUST themselves be subject to access controls and retention policies.

Audit logs MUST NOT contain passwords, tokens or unnecessary personal information.

---

# 48. Testing philosophy

Testing MUST prioritize **realistic integration tests over excessive mocking**.

The project SHOULD prefer:

1. real domain logic;
2. real application services;
3. real PostgreSQL;
4. fake external services where appropriate;
5. mocks only when there is a strong reason to use them.

The goal is to test behavior rather than implementation details.

---

# 49. Database-backed tests

Tests that exercise persistence MUST use an actual PostgreSQL database.

The test environment SHOULD run the same database engine used in production, including PostGIS where relevant.

The tests MUST NOT replace the database with an in-memory substitute.

Tests SHOULD execute the actual SQLx queries against PostgreSQL.

This is particularly important because the application deliberately uses:

* PostgreSQL-specific SQL;
* PostGIS;
* SQLx;
* database constraints;
* transactions.

---

# 50. Transaction isolation for tests

Each database-backed test MUST run inside a transaction.

The basic pattern should be:

```text
BEGIN
   │
   ├── arrange
   ├── execute
   ├── assert
   │
ROLLBACK
```

The transaction MUST be rolled back after the test regardless of whether the test succeeds or fails.

This prevents tests from contaminating one another and avoids requiring extensive cleanup logic.

---

# 51. Transactions inside tests

If the behavior being tested itself requires a database transaction, the test MUST NOT create a second independent transaction on the same connection.

Instead, the test should use a **PostgreSQL SAVEPOINT** to simulate nested transactional behavior.

Conceptually:

```text
Test transaction
│
├── arrange
│
├── SAVEPOINT
│    │
│    └── application transaction
│
├── assertions
│
└── ROLLBACK
```

The implementation plan MUST determine the appropriate SQLx mechanism for exposing this behavior cleanly.

The testing infrastructure SHOULD make this ergonomic rather than requiring every test author to manually manage SQL transaction boilerplate.

---

# 52. Test support crate

The workspace MUST contain a dedicated test-support crate/library, or an equivalent shared testing infrastructure.

Its purpose is to make realistic database-backed tests easy to write.

For example:

```text
test-support/
├── database
├── fixtures
├── builders
└── helpers
```

The exact organization is left to the implementation plan.

---

# 53. Domain-rich test builders

The test-support crate MUST provide helpers/builders for creating realistic domain entities together with their persistent state.

For example:

```rust
let user = TestUser::new(&db)
    .with_email("test@example.com")
    .create()
    .await?;

let parking = TestParkingLocation::new(&db)
    .owned_by(&user)
    .free()
    .indoor()
    .with_cctv()
    .create()
    .await?;
```

The exact API is to be determined during implementation.

The important requirement is that tests should be able to easily construct **domain-rich scenarios** without duplicating SQL setup in every test.

---

# 54. Test builders and persistence

Test fixtures SHOULD:

* create the required database records;
* return useful domain representations;
* support common variations;
* compose with other fixtures;
* use the test transaction;
* avoid hiding important behavior being tested.

For example:

```text
UserBuilder
ParkingBuilder
ReviewBuilder
VerificationBuilder
PhotoBuilder
ReportBuilder
```

Builders SHOULD allow tests to express intent rather than database mechanics.

---

# 55. Fake external services

External services SHOULD generally be represented by **fakes** in automated tests.

Appropriate examples:

```text
FakeGeocoder
FakeEmailProvider
FakeImageStorage
FakeOAuthProvider
FakeMapProvider
```

The fake should provide deterministic behavior and allow tests to inspect interactions where necessary.

For example:

```text
FakeEmailProvider
    ↓
captures sent emails
```

rather than requiring a real email provider.

---

# 56. Avoid excessive mocking

Mocks SHOULD NOT be the default testing strategy.

In particular, tests SHOULD NOT mock:

* PostgreSQL;
* repositories merely to test application services;
* domain objects;
* SQLx;
* simple internal services without a meaningful external boundary.

A test like:

```text
Application → mocked repository → expected method call
```

should generally be avoided when the behavior can instead be tested through:

```text
Application → real repository → PostgreSQL
```

This is especially important because the SQL itself is part of the behavior being tested.

---

# 57. Test layers

The implementation plan SHOULD define at least:

## Domain tests

Fast tests for pure domain behavior.

These should not require PostgreSQL.

## Application/integration tests

Exercise application use cases with:

* real domain objects;
* real SQL persistence;
* fake external providers.

## HTTP tests

Exercise Axum routes and handlers.

Where practical, these should use:

* real application services;
* real database;
* test transactions;
* rendered Askama templates.

## End-to-end/browser tests

The implementation SHOULD introduce browser tests for critical user journeys.

These should be limited to important flows rather than duplicating every application test.

---

# 58. Test naming and behavior

Tests SHOULD describe user-visible/domain behavior.

Prefer:

```text
user_can_verify_that_parking_is_still_available
```

over:

```text
verification_repository_insert_test
```

The test suite should communicate the application's behavior rather than its implementation structure.

---

# 59. Geographic testing

The test-support infrastructure SHOULD make it easy to create parking locations at known coordinates.

For example:

```text
Destination:
Curitiba coordinates

Parking A:
100m away

Parking B:
500m away

Parking C:
2km away
```

This makes proximity, radius and ranking tests deterministic.

Tests SHOULD cover:

* distance filtering;
* radius boundaries;
* ordering;
* nearby duplicate detection;
* PostGIS coordinate behavior;
* spatial-index-compatible queries.

---

# 60. Security and authorization tests

Tests MUST cover authorization boundaries.

Examples:

* user cannot edit another user's private contribution;
* user cannot modify another user's favorite;
* user cannot access another user's private information;
* normal user cannot perform moderator actions;
* unauthenticated users cannot perform authenticated operations;
* suspended users cannot contribute;
* deleted users cannot authenticate;
* users cannot manipulate another user's privacy requests.

These tests SHOULD use the real application/database stack rather than mocking authorization dependencies.

---

# 61. Privacy and data-protection tests

Tests MUST cover privacy requirements.

At minimum:

* user can access their personal data;
* user can export their data;
* user can request account deletion;
* account deletion removes/anonymizes applicable personal information;
* public contributions are no longer attributable to deleted users where required;
* private parking history is not publicly exposed;
* one user cannot export another user's data;
* geolocation is not persisted unexpectedly;
* uploaded image metadata is removed;
* logs do not contain secrets;
* privacy requests are authorized;
* deleted accounts cannot be recovered through old sessions;
* retention jobs correctly remove/anonymize expired data.

---

# 62. Responsive web requirements

The application MUST be responsive and usable from:

* desktop browsers;
* tablet browsers;
* mobile browsers.

There will be no native mobile client initially.

The implementation SHOULD use the same HTML/application architecture across all screen sizes.

---

# 63. Accessibility

The application MUST target **WCAG 2.2 AA**.

The web application SHOULD use:

* semantic HTML;
* keyboard-accessible controls;
* visible focus states;
* appropriate labels;
* accessible forms;
* appropriate ARIA attributes;
* sufficient color contrast;
* accessible error messages;
* accessible dialogs;
* accessible navigation;
* a non-map result representation.

The parking list MUST provide an alternative to interacting with the map.

Critical user journeys SHOULD be tested with keyboard-only navigation.

---

# 64. Security

The application MUST:

* securely hash passwords;
* use secure sessions;
* protect authenticated operations against CSRF;
* validate input server-side;
* escape user-generated content;
* prevent SQL injection through parameterized SQLx queries;
* validate uploaded images;
* enforce upload limits;
* enforce authorization;
* avoid exposing private user information;
* implement appropriate rate limiting;
* use HTTPS in production;
* use secure security headers;
* protect secrets;
* avoid leaking internal errors.

User-generated HTML MUST never be trusted.

The implementation plan MUST define an appropriate Content Security Policy.

---

# 65. Security headers

Production responses SHOULD include appropriate security headers, including where applicable:

```text
Strict-Transport-Security
Content-Security-Policy
X-Content-Type-Options
Referrer-Policy
Permissions-Policy
```

The implementation SHOULD avoid unnecessary third-party scripts.

---

# 66. Privacy and data protection

The application MUST be designed and implemented to support compliance with the applicable requirements of:

* the **EU General Data Protection Regulation (GDPR)**;
* Brazil's **Lei Geral de Proteção de Dados Pessoais (LGPD)**;
* applicable implementing regulations and authoritative guidance.

The implementation MUST NOT treat "GDPR/LGPD compliant" as merely a documentation requirement.

Privacy requirements MUST be reflected in:

* architecture;
* database design;
* authorization;
* data retention;
* logging;
* external-provider integration;
* account lifecycle;
* user interfaces;
* operational processes.

Final legal compliance MUST be validated based on the actual business model, controller/operator relationships, jurisdictions, providers, processing activities and deployment architecture.

The implementation plan MUST explicitly identify:

1. legal/product decisions;
2. technical requirements;
3. operational requirements;
4. requirements requiring legal review.

---

# 67. Data minimization

The application SHOULD collect the minimum personal data necessary for each purpose.

The system MUST NOT collect personal data merely because it might be useful in the future.

The implementation plan MUST identify every personal-data field collected by the application.

The data inventory MUST include at least:

* email address;
* password hash;
* OAuth provider identifiers;
* session information;
* IP address where retained;
* user-agent information where retained;
* reviews;
* contributions;
* verification activity;
* reports;
* photos and metadata;
* browser geolocation;
* audit information.

---

# 68. Personal-data inventory

The implementation plan MUST create a data-processing inventory with at least:

```text
Data element
Purpose
Legal basis
Required/optional
Storage location
Access permissions
Retention period
Recipients
International transfer
Deletion/anonymization behavior
```

No personal-data field should be added without a documented purpose.

---

# 69. Legal basis

The implementation plan MUST identify the applicable legal basis for each category of personal-data processing under both GDPR and LGPD where relevant.

The engineering implementation MUST NOT invent legal bases.

Where the appropriate legal basis is a legal/business decision, the implementation plan MUST mark it explicitly as requiring product/legal review.

Consent MUST NOT be used merely as a generic catch-all.

Where consent is used, it MUST be:

* specific;
* informed;
* freely given;
* recorded where necessary;
* withdrawable.

---

# 70. Privacy notice

The application MUST provide a clear privacy notice.

The privacy notice SHOULD explain:

* controller identity;
* contact information;
* purposes of processing;
* categories of personal data;
* applicable legal bases;
* recipients/processors;
* international transfers;
* retention;
* data-subject rights;
* how to exercise rights;
* security practices at an appropriate level;
* relevant automated decision-making where applicable.

The privacy notice MUST be versioned so that the system can determine which version was presented to a user when necessary.

---

# 71. Terms and policies

The application SHOULD provide:

```text
/privacy
/terms
/cookies
```

The implementation plan MUST determine which policy pages are legally required based on the final deployment and business model.

Legal text MUST be treated as product/legal content rather than generated as an assumed final legal document by the implementation agent.

---

# 72. Data-subject rights

The application MUST provide a mechanism for authenticated users to request applicable privacy rights.

The system SHOULD support:

* access;
* rectification;
* deletion;
* data export/portability where applicable;
* restriction where applicable;
* objection where applicable;
* withdrawal of consent where consent is the legal basis.

The system MUST verify the identity/authority of the requester before disclosing personal information.

Privacy requests MUST be auditable.

The implementation plan MUST define expected response workflows and which requests can be fulfilled automatically versus manually.

---

# 73. Personal-data export

Users MUST be able to request an export of their personal information.

The export SHOULD use a machine-readable format such as JSON and MAY include a human-readable representation.

The export MUST contain only data the requesting user is authorized to receive.

Exports MUST be generated securely.

Download links MUST:

* expire;
* require authentication or an equivalent secure mechanism;
* not be publicly indexable;
* not remain permanently accessible.

---

# 74. Account deletion

Users MUST be able to request deletion of their account.

Deletion MUST distinguish between:

```text
Personal identity
```

and:

```text
Community contributions
```

Where legally permissible and product-appropriate, community contributions MAY be retained after account deletion but MUST be anonymized so they are no longer attributable to the user.

The default model SHOULD be:

```text
Deleted User
    ↓
Remove/anonymize personal identity
    ↓
Retain non-personal community content where appropriate
    ↓
Remove private activity
    ↓
Invalidate sessions
    ↓
Invalidate authentication identities
```

The implementation plan MUST explicitly document which records are:

* deleted;
* anonymized;
* retained;
* legally required to be retained.

---

# 75. Data retention

The application MUST define retention policies for personal and operational data.

At minimum, policies MUST cover:

* inactive accounts;
* deleted accounts;
* sessions;
* password-reset tokens;
* email-verification tokens;
* OAuth identities;
* reviews;
* contribution history;
* reports;
* moderation records;
* audit logs;
* uploaded photos;
* geolocation data;
* "I parked here" events;
* privacy requests;
* security logs.

The default should be **data minimization and deletion when the purpose expires**.

The implementation plan MUST identify all retention periods that require legal/product approval.

Suggested technical defaults:

```text
Password reset token:        1 hour
Email verification token:   24 hours
Session:                     30 days inactive
"I parked here":             90 days
Temporary privacy exports:   24 hours
Temporary upload objects:    24 hours
```

Long-term retention periods for reviews, contributions, reports, audit events and security logs MUST be explicitly decided based on legal and operational requirements.

---

# 76. International data transfers

The implementation plan MUST identify all external providers that may receive personal data.

For each provider, document:

```text
Provider
Purpose
Data transferred
Countries/regions
Processor/controller role
Transfer mechanism
Contract/DPA requirements
Retention
Deletion mechanism
```

This MUST include, where applicable:

* Google;
* email provider;
* geocoding provider;
* map/tile provider;
* object storage provider;
* hosting provider;
* observability provider;
* error-tracking provider.

The architecture SHOULD make it possible to replace providers without changing domain logic.

---

# 77. Third-party privacy boundaries

External providers MUST receive only the minimum information required for their purpose.

For example:

* a map renderer should not receive authenticated user identity;
* a geocoder should not receive account information;
* an image-storage provider should not receive unnecessary user metadata;
* an email provider should receive only information necessary to deliver the email.

The implementation plan MUST document what data crosses each provider boundary.

---

# 78. Cookies

The application MUST distinguish between:

* strictly necessary cookies;
* optional cookies.

The application SHOULD avoid non-essential tracking cookies in the initial version.

The implementation plan MUST identify all cookies created by the application and document:

* purpose;
* lifetime;
* domain/path;
* security flags;
* whether they are first-party or third-party.

Authentication/session cookies are considered application functionality and MUST be handled securely.

If analytics or advertising cookies are introduced later, the consent/privacy architecture MUST be revisited.

---

# 79. Browser geolocation privacy

Browser geolocation MUST NOT be silently persisted.

The application MUST make clear when location is being used.

The application MUST NOT build a permanent location-history database in the initial release.

Any future location-history feature MUST be treated as a new privacy-sensitive product feature.

---

# 80. Photo privacy

Uploaded photos MUST be treated as potentially containing personal information.

The system MUST:

* remove EXIF metadata;
* avoid publishing original files where unnecessary;
* support moderation;
* provide a mechanism to report inappropriate photos;
* avoid exposing uploader email or OAuth identifiers.

The implementation SHOULD consider whether faces, license plates or other personal information are present in photographs.

Automatic detection MAY be introduced later but is not required for the initial release.

---

# 81. Security incidents

The implementation MUST provide enough logging and operational information to investigate security incidents involving personal data.

The implementation plan MUST define:

```text
Detection
 ↓
Classification
 ↓
Containment
 ↓
Impact assessment
 ↓
Personal-data assessment
 ↓
Internal escalation
 ↓
Regulatory/user notification when legally required
 ↓
Remediation
 ↓
Incident record
```

Incident records MUST themselves be protected.

The system SHOULD retain sufficient information to support legally required incident reporting and investigation.

---

# 82. Privacy by design

Privacy MUST be considered during architecture and implementation.

The implementation plan MUST explicitly review:

* data minimization;
* least privilege;
* default privacy settings;
* retention;
* deletion;
* anonymization;
* third-party data sharing;
* browser geolocation;
* photo metadata;
* logs;
* backups;
* exports;
* account deletion.

---

# 83. External geographic services

The application SHOULD use:

* OpenStreetMap-derived geographic data where appropriate;
* MapLibre GL JS for rendering;
* a replaceable tile/style provider;
* a replaceable geocoding provider.

The public OpenStreetMap tile servers and public Nominatim service MUST NOT be treated as unrestricted production infrastructure.

The application SHOULD use a suitable third-party provider or self-host the required services if usage grows.

The implementation plan MUST explicitly document:

* tile provider;
* geocoding provider;
* usage limits;
* licensing;
* attribution requirements;
* caching rules;
* terms of service;
* privacy implications;
* data sent to each provider.

---

# 84. Provider abstraction

The following external concerns SHOULD have explicit boundaries:

```text
Geocoder
MapProvider / MapConfiguration
AuthenticationProvider
EmailProvider
ImageStorage
Clock
TokenGenerator
RateLimiter
```

The application should be able to replace, for example:

```text
Google OAuth
```

with:

```text
Apple
GitHub
Microsoft
...
```

without redesigning the user/account domain.

Likewise, changing geocoding or image storage providers should not require changing parking-domain logic.

---

# 85. Error handling

Errors MUST be represented appropriately across architectural boundaries.

The domain should have domain-relevant errors.

The application layer should expose use-case errors.

The infrastructure layer should translate infrastructure failures.

The web layer should translate application errors into:

* appropriate HTTP status codes;
* user-friendly HTML;
* HTMX-compatible responses.

Internal error details MUST NOT be exposed to users.

Errors MUST be logged appropriately without leaking sensitive data.

---

# 86. Observability

The implementation SHOULD provide structured application logging.

Important operations SHOULD produce useful diagnostic information, including:

* authentication failures;
* external provider failures;
* database errors;
* parking creation;
* moderation actions;
* unexpected application errors;
* privacy requests;
* security events.

Logs MUST NOT contain sensitive information such as:

* passwords;
* authentication tokens;
* OAuth secrets;
* session cookies;
* unnecessary personal information;
* full private user activity.

The implementation plan MUST define log retention.

Diagnostic logs and audit events SHOULD be treated as separate concepts.

---

# 87. Health and readiness

The application SHOULD expose separate health/readiness checks.

At minimum:

```text
Health:
Is the process alive?

Readiness:
Can the application serve requests and access required dependencies?
```

The application MUST be able to distinguish:

```text
database unavailable
```

from:

```text
database available but application has an error
```

---

# 88. Local development environment

The project MUST provide a simple Docker Compose configuration for local development.

The goal is that a developer with Docker installed can clone the repository and start the required local infrastructure with:

```bash
docker compose up -d
```

The Docker Compose environment MUST provide all infrastructure dependencies required for normal local development and testing, including at minimum:

* PostgreSQL;
* PostGIS.

If other infrastructure becomes necessary during implementation, it SHOULD be included in Docker Compose where practical.

Examples might include:

* local object storage for images;
* email testing service;
* other supporting services.

External services that require credentials, such as Google OAuth or a production geocoding provider, MAY remain external.

---

# 89. Database initialization

The local PostgreSQL container MUST be initialized with PostGIS enabled.

The development workflow SHOULD make database setup automatic.

A developer SHOULD NOT need to manually:

```text
create database
install PostGIS
create extensions
create users
run migrations
```

after starting the environment.

---

# 90. Database migrations

The project MUST provide an easy mechanism to run database migrations against the Docker Compose PostgreSQL instance.

The preferred developer workflow SHOULD be similar to:

```bash
docker compose up -d
cargo run -- migrate
```

or an equivalent project command.

The exact command should be determined during the implementation plan.

---

# 91. Environment configuration

The repository MUST provide:

```text
.env.example
```

It SHOULD contain all required local-development configuration, including:

* database connection;
* application port;
* session configuration;
* Google OAuth configuration;
* external provider configuration;
* image storage configuration;
* email configuration;
* rate-limit configuration where applicable.

Secrets MUST NOT be committed to the repository.

Local development SHOULD work with sensible defaults wherever credentials are not strictly required.

---

# 92. Developer onboarding

The implementation SHOULD aim for a simple onboarding process:

```text
git clone ...
cd application

docker compose up -d

cargo run ...
```

A new developer SHOULD NOT need to understand the production infrastructure in order to run the application locally.

The implementation plan MUST document the complete local-development workflow.

The README MUST document:

* prerequisites;
* setup;
* environment variables;
* database initialization;
* migrations;
* running the application;
* running tests;
* resetting local data;
* optional external services.

---

# 93. Test database

The test suite SHOULD use PostgreSQL running through the same Docker Compose environment, or an equivalent disposable PostgreSQL instance.

The test database MUST use the same PostgreSQL/PostGIS versions and extensions expected by the application.

The testing infrastructure SHOULD make it easy to run:

```bash
cargo test
```

without manually creating or configuring a database first.

The implementation plan MUST explain how the test suite obtains and prepares its database connection.

---

# 94. Persistence and local data

Docker Compose SHOULD use a named Docker volume for PostgreSQL during normal development so that restarting containers does not unnecessarily destroy the developer's local database.

Tests MUST nevertheless remain isolated through the previously specified **transaction-per-test + rollback** strategy.

A developer SHOULD have an easy way to completely reset the local database when needed.

For example:

```bash
docker compose down -v
docker compose up -d
```

should result in a clean local database.

---

# 95. Local service health

Docker Compose SHOULD define health checks for infrastructure services.

The application SHOULD consider service startup ordering and readiness rather than merely container startup.

The implementation plan MUST document how developers know that infrastructure is ready.

---

# 96. Docker Compose philosophy

Docker Compose is intended primarily for **local development**, not necessarily as the production deployment mechanism.

The Compose configuration SHOULD optimize for:

* simplicity;
* fast startup;
* reproducibility;
* easy onboarding;
* easy database reset;
* easy testing.

Production deployment architecture should remain independent of Docker Compose unless there is a deliberate reason to reuse it.

---

# 97. Deployment architecture

The implementation plan MUST define a production deployment architecture.

At minimum, it MUST address:

* application hosting;
* PostgreSQL/PostGIS;
* object storage;
* TLS;
* secrets;
* email;
* OAuth;
* map/geocoding providers;
* backups;
* database migrations;
* observability;
* health checks;
* deployment strategy;
* rollback strategy.

The production database MUST have automated backups.

The implementation plan MUST define:

* backup frequency;
* retention;
* encryption;
* restore procedure;
* recovery objectives.

The implementation MUST NOT assume that backups alone constitute a disaster-recovery strategy.

---

# 98. Data deletion and backups

The implementation plan MUST consider the interaction between deletion requests and backups.

Deleted/anonymized data MUST not normally be restored into active production state merely because an old backup is restored.

The plan MUST document:

* backup retention;
* restoration procedures;
* handling of deleted accounts after restore;
* operational controls for backup access.

---

# 99. Performance requirements

The implementation plan MUST identify reasonable initial performance targets.

Recommended defaults:

```text
Typical server-rendered page:
target < 500ms application time

Nearby search:
target < 300ms database/application time under normal load

Simple authenticated mutation:
target < 500ms application time under normal load
```

These are engineering targets, not strict SLA guarantees.

The implementation MUST avoid premature optimization while ensuring that geographic queries use appropriate indexes.

---

# 100. Concurrency

The implementation plan MUST identify operations where concurrent modifications are possible.

At minimum consider:

* parking edits;
* proposed changes;
* reviews;
* favorites;
* moderation;
* verification;
* account deletion.

The database MUST enforce invariants that cannot safely be enforced only in application code.

Where appropriate, use:

* unique constraints;
* transactions;
* optimistic concurrency;
* row locking.

The implementation plan should explain the chosen strategy.

---

# 101. Search consistency

The initial release SHOULD favor correctness and simplicity over advanced search infrastructure.

PostgreSQL/PostGIS SHOULD remain the primary search engine.

A separate search engine MUST NOT be introduced unless justified by measured requirements.

The implementation plan MUST define whether text search uses:

* PostgreSQL full-text search;
* trigram indexes;
* provider search;
* or a combination.

---

# 102. Internationalization

The initial release SHOULD support at least:

```text
Portuguese (Brazil)
English
```

The architecture SHOULD avoid hard-coding user-facing strings into domain/application logic.

The implementation plan MUST determine whether internationalization is implemented in the first release or prepared for future implementation.

Dates, times, currency and measurement units SHOULD be rendered appropriately for the user's locale where practical.

---

# 103. Content moderation

User-generated content MUST be treated as untrusted.

This includes:

* parking names;
* descriptions;
* reviews;
* report descriptions;
* proposed changes;
* image uploads.

The application MUST:

* escape rendered content;
* validate lengths;
* prevent HTML injection;
* limit excessive content;
* provide moderation mechanisms.

---

# 104. External navigation

The application MUST NOT implement turn-by-turn navigation.

Instead, it SHOULD provide an external navigation action.

The implementation plan MUST define how links are generated for supported navigation providers.

The application SHOULD avoid sending unnecessary personal information to navigation providers.

---

# 105. Recommendation transparency

The application SHOULD provide a basic explanation for why a parking location is recommended.

For example:

```text
Recommended because:
- 180m away
- indoor
- CCTV
- verified 12 days ago
- 4.7/5 rating
```

The recommendation system MUST NOT claim certainty that cannot be supported by the underlying data.

---

# 106. Information confidence

The system SHOULD distinguish between:

```text
Reported
Verified
Recently verified
Stale
Conflicting
```

where appropriate.

Conflicting verification signals SHOULD NOT simply be averaged away without preserving the underlying information.

The implementation plan MUST define how conflicting community information is resolved.

---

# 107. Data history

Important parking information changes SHOULD preserve history.

The implementation MUST determine which fields require historical tracking.

At minimum, history SHOULD be retained for:

* existence;
* location;
* type;
* cost;
* opening hours;
* security;
* moderation state.

Historical records MUST not unnecessarily expose personal information.

---

# 108. API philosophy

The initial application SHOULD NOT expose a public API.

Internal application boundaries SHOULD still use well-defined use cases and interfaces.

A future public API MUST be treated as a separate product/security decision.

---

# 109. Scraping and automated access

The implementation SHOULD provide reasonable protections against abusive automated access.

The application MAY use:

* rate limiting;
* robots.txt where appropriate;
* caching;
* request throttling.

Public parking information MAY be indexed by search engines where desired.

Private user information MUST never be exposed through publicly indexable pages.

---

# 110. SEO

Public parking pages SHOULD be server-rendered and indexable where product-appropriate.

The implementation SHOULD provide:

* meaningful page titles;
* metadata;
* canonical URLs;
* semantic HTML;
* sitemap support;
* robots.txt.

Private account pages MUST not be indexed.

---

# 111. URL design

Public parking locations SHOULD have stable URLs.

For example:

```text
/parking/{id}
```

or an equivalent stable structure.

URLs SHOULD NOT contain unnecessary personal information.

Search/filter state SHOULD be representable through query parameters where useful.

---

# 112. Implementation-plan requirements

The next agent should **not immediately implement the application**.

It should first create a detailed implementation plan based on this specification.

The plan MUST address:

1. Cargo workspace structure.
2. Crate responsibilities and dependency direction.
3. Domain model.
4. Application/use-case architecture.
5. PostgreSQL schema.
6. PostGIS schema and geographic queries.
7. SQLx organization.
8. Migration strategy.
9. Authentication architecture.
10. Email/password authentication.
11. Email verification.
12. Google OAuth.
13. Extensible authentication providers.
14. Session management.
15. Account lifecycle.
16. Account deletion/anonymization.
17. Axum routing.
18. HTMX interaction architecture.
19. Askama template architecture.
20. Alpine.js usage.
21. MapLibre integration.
22. Geocoding provider.
23. Map/tile provider.
24. Image storage.
25. Image-processing/security pipeline.
26. Authorization.
27. Rate limiting.
28. Abuse prevention.
29. Moderation.
30. Contribution history.
31. Audit events.
32. Privacy architecture.
33. GDPR requirements.
34. LGPD requirements.
35. Data-processing inventory.
36. Legal-basis mapping.
37. Data retention.
38. Data deletion/anonymization.
39. Personal-data export.
40. Privacy-request workflow.
41. Cookie strategy.
42. International data transfers.
43. Third-party provider data flows.
44. Security strategy.
45. Security headers.
46. Incident-response strategy.
47. Test-support crate.
48. Database transaction-per-test strategy.
49. SAVEPOINT strategy for transactional application behavior.
50. Domain-rich test builders.
51. Fake external services.
52. Integration-test strategy.
53. HTTP testing strategy.
54. Privacy/security testing.
55. Browser/E2E testing.
56. Accessibility strategy.
57. Search/pagination strategy.
58. Recommendation algorithm.
59. Moderation state machines.
60. Audit strategy.
61. Observability.
62. Deployment architecture.
63. Database backups.
64. Disaster recovery.
65. Local development environment.
66. Docker Compose architecture.
67. Dependency list and `cargo add` commands.
68. Performance considerations.
69. Internationalization.
70. Risks and architectural decisions.
71. Any requirements that remain ambiguous.
72. Any requirements requiring legal/business approval.

The implementation plan MUST clearly distinguish:

```text
Requirement already decided
        ↓
Implementation decision
        ↓
Reasoning/tradeoff
        ↓
Remaining product/legal decision
```

The implementation agent MUST NOT silently invent major requirements when the specification explicitly leaves a legal or product decision unresolved.

---

# 113. Most important architectural constraint

The resulting system should make this kind of test natural:

```text
Test
 │
 ├── begin transaction
 │
 ├── create realistic domain state
 │
 ├── invoke real application use case
 │
 ├── execute real SQLx queries
 │
 ├── assert resulting domain state
 │
 └── rollback
```

rather than:

```text
Test
 │
 ├── mock repository
 ├── mock database
 ├── mock service
 ├── mock provider
 └── assert mock calls
```

**The former should be the default philosophy for this project.**

---

# 114. Definition of done for the initial release

The implementation MUST NOT be considered complete merely because the application compiles.

The initial release is complete when:

## Functionality

* users can register;
* users can verify email;
* users can authenticate;
* Google OAuth works;
* users can search for destinations;
* nearby parking can be discovered;
* parking can be filtered/sorted;
* parking details work;
* external navigation works;
* authenticated users can contribute;
* reviews work;
* verification works;
* favorites work;
* reports work;
* moderation works;
* photos work;
* stale information is identified;
* account deletion works;
* personal-data export works.

## Architecture

* Clean Architecture boundaries are respected;
* domain does not depend on infrastructure;
* SQLx uses explicit SQL;
* PostgreSQL/PostGIS are used correctly;
* external providers are abstracted;
* HTMX handles server-driven interactions;
* Alpine.js is limited to local UI state;
* Askama renders HTML.

## Security

* passwords are securely hashed;
* sessions are secure;
* CSRF protection works;
* authorization is enforced;
* rate limiting exists;
* uploads are validated;
* EXIF is removed;
* secrets are protected;
* security headers are configured;
* sensitive information is absent from logs.

## Privacy

* personal-data inventory exists;
* privacy policy exists;
* retention rules exist;
* account deletion works;
* anonymization works where applicable;
* personal-data export works;
* privacy requests are handled;
* third-party data flows are documented;
* international transfers are documented;
* GDPR/LGPD technical requirements have been reviewed;
* unresolved legal decisions are explicitly identified.

## Testing

* domain tests exist;
* integration tests use real PostgreSQL/PostGIS;
* tests use real SQLx queries;
* transaction-per-test works;
* SAVEPOINT strategy works;
* test builders exist;
* external providers have fakes;
* authorization tests exist;
* privacy tests exist;
* critical E2E flows exist.

## Operations

* Docker Compose works;
* a new developer can run the project locally;
* migrations are reproducible;
* health checks exist;
* production deployment is documented;
* backups are configured;
* restore procedures are documented;
* observability is available.

---

# 115. Final implementation principle

The implementation should favor:

```text
Simple
Explicit
Testable
Secure
Privacy-preserving
Replaceable
Observable
```

over:

```text
Highly abstract
Highly generic
Over-engineered
Mock-heavy
Client-side heavy
Provider-specific
```

The application should remain a relatively small, understandable Rust web application.

Do not introduce infrastructure, frameworks, abstractions or services without a concrete requirement or measurable benefit.

---

# 116. Resolved decisions (addendum)

The following decisions were resolved during requirements review. They are the single source of truth for these points and are reflected in the sections noted.

1. **Initial data source** — There is NO seed/imported parking dataset. Production starts empty and is populated entirely by user contributions. Consequence: search-over-empty MUST degrade gracefully with an explicit "no results near here — add the first parking location" call-to-action. For development/testing milestones, temporary mock data MAY be introduced; every such dataset MUST be tracked in `PLAN.md`'s ledger and removed or gated behind a dev flag before production. (See sections 2 and 21.)

2. **Photo moderation** — Photos are reviewed by moderators/administrators through a moderation dashboard before publication. Photos therefore have a `PENDING_REVIEW → APPROVED → REJECTED` lifecycle and are not publicly visible until approved. (See sections 2 and 30.)

3. **Role bootstrap and assignment** — The initial ADMIN account is seeded (CLI command or idempotent migration). ADMIN users grant/revoke MODERATOR and ADMIN roles. Role changes are audited and deny-by-default. (See section 19.)

4. **Timezone handling** — Event timestamps are stored as UTC and rendered in the viewer's timezone. Opening hours are stored as wall-clock ranges with the parking location's IANA timezone (derived from coordinates), never converted to UTC, and "open now" is computed in the location's timezone. (See sections 8, 24 and 29.)

5. **Expected scale/volume** — Not yet defined. Performance targets in section 99 remain the operating constraints; volume assumptions are left TBD and MUST be revisited before capacity planning.

6. **HTMX version** — HTMX 4 is the target (https://four.htmx.org/docs/whats-new-in-htmx-4). Note that htmx 4 swaps `4xx`/`5xx` error responses by default; error-handling fragments MUST be designed accordingly (see section 85). Boosted navigation uses `hx-boost` with the `hx-alpine-compat` extension so Alpine re-initializes after swaps.

7. **Internationalization** — implemented from the first search milestone (M1), not deferred: a bilingual (pt-BR + en) catalog with per-request locale resolution (`Accept-Language`, fallback pt-BR) overridable by a `lang` cookie toggle. User-facing strings live in the web-layer catalog, not hard-coded in domain/application logic. (See section 102.)

8. **Text search** — none. Search resolves a *destination* (address/place/landmark/neighborhood/city/current location) via the geocoder to coordinates, then runs a PostGIS proximity query. There is no free-text search over parking names/descriptions and no separate search engine in the initial release. (See sections 21 and 101.)

9. **Image storage** — abstracted behind an `ObjectStorage` port; the development implementation is local-disk with HMAC-signed, expiring URLs, replaceable by S3-compatible storage without domain changes. Introduced early (M1) to back seeded/location photos and the details gallery; the full upload → validate → re-encode → EXIF-strip → thumbnail → moderate pipeline remains M4. (See sections 30 and 84.)

10. **Security-attribute catalog** — attribute **codes** are a hardcoded domain list; **labels** are resolved via i18n in the presentation layer, not stored in a database catalog table (localizable per §102). Per-location values remain in `parking_security` as (code, tri-state). (See sections 8 and 28.)
