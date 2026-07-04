# Horary Calculator — Specifications

## Functional Spec

### What it does
A browser-based horary astrology chart calculator. Given a date, time, and geographic location, it calculates and displays an astrological chart using the Regiomontanus house system.

### User flows

**Default (current moment):**
1. App opens. Browser geolocation is requested automatically.
2. If granted: date/time defaults to now, location is set from GPS, city name is reverse-geocoded and displayed. Chart renders immediately.
3. If denied: "Detect my location" button appears. User can retry, or manually search a city or enter coordinates. Chart renders once a location is set.

**Look up past question:**
1. User opens Settings (⚙ icon) and checks "Look up past question".
2. Date, time, and location fields appear for manual entry.
3. User can enter a question for reference.
4. "Use current time & place" button resets fields to now + detected location.

### Inputs
- **Date**: calendar date (any past or future date)
- **Time**: 12-hour (AM/PM) or 24-hour depending on settings
- **Location**: city search (via Nominatim) or explicit DMS coordinates
- **Question**: free-text memo, not used in calculation

### Outputs

| Section | Contents |
|---|---|
| Chart wheel | SVG astrological wheel with planets and aspect lines |
| Angles | ASC, DSC, MC, IC with sign and degree |
| Houses | 12 house cusps with sign and degree |
| Planets | 10 planets (Sun–Pluto) with sign and degree |
| Aspects list | Aspect type, planets involved, orb, applying/separating |
| Aspects chart | Grid showing all pairwise aspects |

All output sections are hidden by default and toggled via Settings checkboxes. Settings persist across page refreshes via localStorage.

### Settings
| Setting | Default | Persisted |
|---|---|---|
| Look up past question | off | no |
| Show angles | off | yes |
| Show houses | off | yes |
| Show planets | off | yes |
| Show aspects list | off | yes |
| Show aspects chart | off | yes |
| Show all | — | — |
| Use 24-hour time | off | yes |

"Show all" is a convenience toggle that sets all show-checkboxes at once.

### Aspect orbs
All five aspects use a ±5° orb (configured as `orbit: 10` in the library, which treats the value as full width):

| Aspect | Angle |
|---|---|
| Conjunction | 0° |
| Sextile | 60° |
| Square | 90° |
| Trine | 120° |
| Opposition | 180° |

---

## Technical Spec

### Stack
- **Framework**: React 19 + TypeScript, built with Vite
- **Deployment**: GitHub Pages at `/horary-web/`
- **Astrology engine**: `circular-natal-horoscope-js` (Regiomontanus houses, planetary positions)
- **Chart rendering**: `@astrodraw/astrochart` (SVG wheel + aspect calculator)
- **Geocoding**: Nominatim (OpenStreetMap) — city search and reverse geocoding
- **Timezone lookup**: timeapi.io — coordinate-to-timezone conversion
- **PWA**: Web App Manifest + service worker (`public/sw.js`)

### Key files
| File | Role |
|---|---|
| `src/chartCalc.ts` | All calculation logic; pure functions; no React |
| `src/App.tsx` | All UI state and rendering |
| `src/ChartWheel.tsx` | Wrapper around `@astrodraw/astrochart` |
| `src/AspectGrid.tsx` | Pairwise aspect grid component |
| `public/sw.js` | Service worker: network-first cache strategy |
| `public/manifest.json` | PWA manifest |

### Calculation pipeline (`chartCalc.ts`)
1. Validate inputs (date, lat/lon bounds)
2. Create `Origin` from local datetime components (year/month/date/hour/minute, **not** UTC)
3. Create `Horoscope` with tropical zodiac, Regiomontanus houses
4. Extract house cusps and planet positions as decimal degrees
5. Compute aspects via `AspectCalculator.radix()`; deduplicate by sorted pair + type
6. Determine applying/separating using mean daily motion speeds (hardcoded table)
7. Return `ChartSummary` object

### Coordinate input
Coordinates are entered as DMS (degrees/minutes/seconds) with N/S/E/W. Converted to signed decimal via `dmsToDecimal()` before calculation. City search populates DMS fields from Nominatim lat/lon.

### Service worker behaviour
- **On localhost**: SW is never registered. If one is already installed (from a previous session), it is unregistered on page load and the page reloads once to clear it.
- **On production**: SW is registered. Uses network-first strategy: attempts live fetch, caches successful responses, falls back to cache on network failure. On SW activation, old caches are deleted and all open tabs are hard-navigated to pick up the new bundle.
- Cache version: bump `CACHE` constant in `sw.js` on every deployment that changes JS assets.

### Settings persistence
Each show-checkbox and the 24-hour toggle write directly to `localStorage` in their `onChange` handler (synchronous, not via `useEffect`). Keys: `showAngles`, `showHouses`, `showPlanets`, `showAspects`, `showAspectGrid`, `use24Hour`. Values: `'true'` / `'false'`. Read back via lazy `useState` initializers on mount.

### CI/CD
Tests (`npm test`) must pass before the build job runs. Build (`npm run build`) must pass before deployment. If either fails, deployment is blocked and GitHub emails the repository owner.

Pipeline: `test → build → deploy`
