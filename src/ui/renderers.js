import { signRuler } from '../astro/dignities.js';
import { PLANET_GLYPHS, signInfo, norm360 } from '../astro/utils.js';

export function renderChartWheel(chart, containerId) {
    const container = document.getElementById(containerId);
    if (!container) return;

    const size = 560;
    const cx = size / 2;
    const cy = size / 2;
    const rOuter = size / 2 - 10;
    const rSignOuter = rOuter;
    const rSignInner = rOuter - 32;
    const rHouseOuter = rSignInner;
    const rHouseInner = rHouseOuter * 0.38;
    const rPlanet = (rSignInner + rHouseInner) / 2 + 15;
    const rAspect = rHouseInner - 5;

    const { positions, houses, aspects } = chart;
    const asc = houses.asc;
    const titleId = `${containerId}-title`;
    const descId = `${containerId}-desc`;

    const toAngle = (lon) => (norm360(lon - asc)) * Math.PI / 180 + Math.PI;
    const polar = (r, angle) => [cx + r * Math.cos(angle), cy - r * Math.sin(angle)];

    let svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${size} ${size}" role="img" aria-labelledby="${titleId}" aria-describedby="${descId}">`;
    svg += `<title id="${titleId}">Astrological chart wheel</title>`;
    svg += `<desc id="${descId}">A circular chart showing zodiac signs, house cusps, planetary positions, and major aspects.</desc>`;
    svg += `<defs>
    <filter id="glow"><feGaussianBlur stdDeviation="2" result="c"/><feMerge><feMergeNode in="c"/><feMergeNode in="SourceGraphic"/></feMerge></filter>
  </defs>`;

    svg += `<circle cx="${cx}" cy="${cy}" r="${rOuter}" fill="#0a0a1a" stroke="rgba(201,168,76,0.2)" stroke-width="1.5"/>`;

    const elementColors = { fire: '#e05555', earth: '#7ea05a', air: '#5a9ee0', water: '#5ac0c0' };
    const elementBackgrounds = {
        fire: 'rgba(224,85,85,0.08)',
        earth: 'rgba(126,160,90,0.08)',
        air: 'rgba(90,158,224,0.08)',
        water: 'rgba(90,192,192,0.08)',
    };

    for (let i = 0; i < 12; i++) {
        const startLon = i * 30;
        const endLon = (i + 1) * 30;
        const a1 = toAngle(startLon);
        const a2 = toAngle(endLon);
        const info = signInfo(startLon);
        const p1o = polar(rSignOuter, a1);
        const p2o = polar(rSignOuter, a2);
        const p1i = polar(rSignInner, a1);
        const p2i = polar(rSignInner, a2);
        const largeArc = 0;

        svg += `<path d="M ${p1o[0]} ${p1o[1]} A ${rSignOuter} ${rSignOuter} 0 ${largeArc} 1 ${p2o[0]} ${p2o[1]} L ${p2i[0]} ${p2i[1]} A ${rSignInner} ${rSignInner} 0 ${largeArc} 0 ${p1i[0]} ${p1i[1]} Z" fill="${elementBackgrounds[info.element]}" stroke="rgba(201,168,76,0.15)" stroke-width="0.5"/>`;

        const midAngle = toAngle(startLon + 15);
        const gp = polar((rSignOuter + rSignInner) / 2, midAngle);
        svg += `<text x="${gp[0]}" y="${gp[1]}" text-anchor="middle" dominant-baseline="central" fill="${elementColors[info.element]}" font-size="15" font-family="serif" opacity="0.9">${info.glyph}</text>`;
        svg += `<line x1="${p1i[0]}" y1="${p1i[1]}" x2="${p1o[0]}" y2="${p1o[1]}" stroke="rgba(201,168,76,0.12)" stroke-width="0.5"/>`;
    }

    svg += `<circle cx="${cx}" cy="${cy}" r="${rSignInner}" fill="none" stroke="rgba(201,168,76,0.25)" stroke-width="1"/>`;
    svg += `<circle cx="${cx}" cy="${cy}" r="${rHouseInner}" fill="#060612" stroke="rgba(201,168,76,0.15)" stroke-width="0.5"/>`;

    for (let i = 0; i < 12; i++) {
        const angle = toAngle(houses.cusps[i]);
        const inner = polar(rHouseInner, angle);
        const outer = polar(rSignInner, angle);

        const isAngle = i === 0 || i === 3 || i === 6 || i === 9;
        const color = isAngle ? 'rgba(201,168,76,0.6)' : 'rgba(201,168,76,0.15)';
        const width = isAngle ? 1.5 : 0.7;

        svg += `<line x1="${inner[0]}" y1="${inner[1]}" x2="${outer[0]}" y2="${outer[1]}" stroke="${color}" stroke-width="${width}"/>`;

        const nextCusp = houses.cusps[(i + 1) % 12];
        let diff = norm360(nextCusp - houses.cusps[i]);
        if (diff < 0.1) diff = 1;

        const midLon = houses.cusps[i] + diff / 2;
        const numAngle = toAngle(midLon);
        const np = polar(rHouseInner + 22, numAngle);
        svg += `<text x="${np[0]}" y="${np[1]}" text-anchor="middle" dominant-baseline="central" fill="rgba(201,168,76,0.5)" font-size="11" font-weight="600" font-family="Inter,sans-serif">${i + 1}</text>`;
    }

    const ascAngle = toAngle(houses.asc);
    const ascP = polar(rSignOuter + 2, ascAngle);
    svg += `<text x="${ascP[0] - 18}" y="${ascP[1]}" text-anchor="middle" fill="#c9a84c" font-size="11" font-weight="600" font-family="Cinzel,serif">ASC</text>`;

    const mcAngle = toAngle(houses.mc);
    const mcP = polar(rSignOuter + 2, mcAngle);
    svg += `<text x="${mcP[0]}" y="${mcAngle > Math.PI && mcAngle < 2 * Math.PI ? mcP[1] + 15 : mcP[1] - 10}" text-anchor="middle" fill="#c9a84c" font-size="11" font-weight="600" font-family="Cinzel,serif">MC</text>`;

    for (const asp of aspects) {
        if (!asp.major) continue;
        const a1 = toAngle(asp.lon1);
        const a2 = toAngle(asp.lon2);
        const p1 = polar(rAspect, a1);
        const p2 = polar(rAspect, a2);
        const opacity = asp.exact ? 0.6 : Math.max(0.1, 0.4 - asp.orb / 20);
        svg += `<line x1="${p1[0]}" y1="${p1[1]}" x2="${p2[0]}" y2="${p2[1]}" stroke="${asp.color}" stroke-width="${asp.exact ? 1.5 : 0.8}" opacity="${opacity}"/>`;
    }

    const placed = [];
    for (const [name, pos] of Object.entries(positions)) {
        if (name === 'SouthNode') continue;
        let angle = toAngle(pos.longitude);
        let nudge = 0;
        let point = polar(rPlanet, angle);
        let safety = 0;
        while (safety < 15) {
            let collision = false;
            for (const prev of placed) {
                const dist = Math.sqrt((point[0] - prev[0]) ** 2 + (point[1] - prev[1]) ** 2);
                if (dist < 18) {
                    collision = true;
                    break;
                }
            }
            if (!collision) break;
            nudge += 0.09;
            point = polar(rPlanet, angle + nudge);
            safety++;
        }
        angle += nudge;
        placed.push(point);

        const pp = polar(rPlanet, angle);
        const glyph = PLANET_GLYPHS[name] || '?';
        const color = pos.retrograde ? '#e05555' : '#e0dde8';
        const tp = polar(rSignInner - 2, toAngle(pos.longitude));
        const tp2 = polar(rPlanet + 12, toAngle(pos.longitude));
        svg += `<line x1="${tp[0]}" y1="${tp[1]}" x2="${tp2[0]}" y2="${tp2[1]}" stroke="rgba(224,221,232,0.2)" stroke-width="0.5"/>`;
        svg += `<text x="${pp[0]}" y="${pp[1]}" text-anchor="middle" dominant-baseline="central" fill="${color}" font-size="14" filter="url(#glow)" style="cursor:pointer" data-planet="${name}">${glyph}</text>`;

        const dp = polar(rPlanet - 16, angle);
        const si = signInfo(pos.longitude);
        svg += `<text x="${dp[0]}" y="${dp[1]}" text-anchor="middle" dominant-baseline="central" fill="rgba(224,221,232,0.5)" font-size="7.5" font-family="Inter,sans-serif">${si.degree}°${si.glyph}</text>`;
    }

    svg += `</svg>`;
    container.innerHTML = svg;
}

export function renderPositionsTable(chart) {
    const tbody = document.getElementById('positionsBody');
    if (!tbody) return;

    let html = '';
    for (const [name, pos] of Object.entries(chart.positions)) {
        if (name === 'SouthNode') continue;
        const si = signInfo(pos.longitude);
        const glyph = PLANET_GLYPHS[name] || '';
        const dignity = chart.dignities[name];
        const dignityText = dignity ? dignityLabel(dignity) : '';
        const accidentalText = accidentalDignityLabel(chart.accidentalDignities?.[name]);
        const retroClass = pos.retrograde ? ' retrograde' : '';
        const dignityMarkup = [dignityText, accidentalText].filter(Boolean).join('<br>');

        html += `<tr>
      <td style="font-size:1.2rem">${glyph}</td>
      <th scope="row" class="${retroClass}">${name}${pos.retrograde ? '' : ''}</th>
      <td>${si.degree}° ${si.glyph} ${String(si.minutes).padStart(2, '0')}'</td>
      <td><span class="sign-badge ${si.element}">${si.glyph} ${si.name}</span></td>
      <td>${pos.house || '-'}</td>
      <td style="color:${pos.speed >= 0 ? 'var(--text-dim)' : 'var(--danger)'}">${pos.speed >= 0 ? '' : ''}${pos.speed?.toFixed(3) ?? '-'}°/d</td>
      <td>${dignityMarkup}</td>
    </tr>`;
    }
    tbody.innerHTML = html;
}

export function dignityLabel(d) {
    const parts = [];
    if (d.domicile) parts.push('<span style="color:var(--success)">Dom</span>');
    if (d.exaltation) parts.push('<span style="color:var(--success)">Exalt</span>');
    if (d.triplicityRuler) parts.push('<span style="color:var(--air)">Trip</span>');
    if (d.termRuler) parts.push('<span style="color:var(--text-dim)">T</span>');
    if (d.faceRuler) parts.push('<span style="color:var(--text-dim)">F</span>');
    if (d.detriment) parts.push('<span style="color:var(--danger)">Det</span>');
    if (d.fall) parts.push('<span style="color:var(--danger)">Fall</span>');
    if (d.peregrine && parts.length === 0) parts.push('<span style="color:var(--text-muted)">Per</span>');
    return parts.join(' ');
}

export function accidentalDignityLabel(d) {
    if (!d) return '';
    const parts = [];
    if (d.angularity === 'angular') parts.push('<span style="color:var(--success)">Angular</span>');
    if (d.angularity === 'succedent') parts.push('<span style="color:var(--text-dim)">Succedent</span>');
    if (d.angularity === 'cadent') parts.push('<span style="color:var(--danger)">Cadent</span>');
    if (d.retrograde) parts.push('<span style="color:var(--danger)">Rx</span>');
    if (d.solarCondition === 'cazimi') parts.push('<span style="color:var(--success)">Cazimi</span>');
    if (d.solarCondition === 'combust') parts.push('<span style="color:var(--danger)">Combust</span>');
    if (d.solarCondition === 'underBeams') parts.push('<span style="color:var(--danger)">Under beams</span>');
    if (d.inJoy) parts.push('<span style="color:var(--air)">Joy</span>');
    if (Number.isFinite(d.score)) {
        const scoreClass = d.score >= 0 ? 'var(--text-dim)' : 'var(--danger)';
        parts.push(`<span style="color:${scoreClass}">Acc ${d.score >= 0 ? '+' : ''}${d.score}</span>`);
    }
    return parts.join(' ');
}

export function renderCuspsTable(chart) {
    const tbody = document.getElementById('cuspsBody');
    if (!tbody) return;

    let html = '';
    const ordinal = ['I', 'II', 'III', 'IV', 'V', 'VI', 'VII', 'VIII', 'IX', 'X', 'XI', 'XII'];
    for (let i = 0; i < 12; i++) {
        const lon = chart.houses.cusps[i];
        const si = signInfo(lon);
        const ruler = signRuler(si.index);
        const rulerGlyph = PLANET_GLYPHS[ruler] || '';
        const isAngle = i === 0 || i === 3 || i === 6 || i === 9;
        const angleLabel = i === 0 ? ' (ASC)' : i === 9 ? ' (MC)' : i === 6 ? ' (DSC)' : i === 3 ? ' (IC)' : '';

        html += `<tr style="${isAngle ? 'font-weight:600;color:var(--gold)' : ''}">
      <th scope="row">${ordinal[i]}${angleLabel}</th>
      <td>${si.degree}° ${si.glyph} ${String(si.minutes).padStart(2, '0')}'</td>
      <td><span class="sign-badge ${si.element}">${si.glyph} ${si.name}</span></td>
      <td>${rulerGlyph} ${ruler || ''}</td>
    </tr>`;
    }
    tbody.innerHTML = html;
}

export function renderAspectGrid(chart) {
    const container = document.getElementById('aspectGridContainer');
    if (!container) return;

    const planets = Object.keys(chart.positions).filter(n => n !== 'SouthNode');
    const glyphs = planets.map(n => PLANET_GLYPHS[n] || n[0]);
    const n = planets.length;
    const aspectMap = {};
    for (const asp of chart.aspects) {
        const key = `${asp.planet1}|${asp.planet2}`;
        aspectMap[key] = asp;
        aspectMap[`${asp.planet2}|${asp.planet1}`] = asp;
    }

    let html = '<table class="aspect-grid-table" style="table-layout:fixed;width:100%;border-collapse:collapse;margin-top:0.5rem">';
    html += '<caption class="sr-only">Aspect grid between chart bodies</caption>';
    html += '<tr><td style="width:2.5rem"></td>';
    for (let i = 0; i < n; i++) {
        html += `<th scope="col" title="${planets[i]}" style="text-align:center;font-size:1.1rem;padding:0.4rem 0;color:var(--text-dim);font-weight:400">${glyphs[i]}</th>`;
    }
    html += '</tr>';

    for (let i = 0; i < n; i++) {
        html += `<tr><th scope="row" title="${planets[i]}" style="font-size:1.1rem;padding:0.4rem 0.2rem;color:var(--text-dim);border-right:1px solid rgba(201,168,76,0.1);font-weight:400">${glyphs[i]}</th>`;
        for (let j = 0; j < n; j++) {
            if (j < i) {
                const asp = aspectMap[`${planets[i]}|${planets[j]}`];
                if (asp) {
                    const phase = asp.applying === true ? ', applying' : asp.separating === true ? ', separating' : '';
                    html += `<td class="aspect-cell" title="${planets[i]} ${asp.aspectName} ${planets[j]} (${asp.orb.toFixed(2)}°${phase})" style="text-align:center;padding:0.4rem 0;border-right:1px solid rgba(201,168,76,0.05);border-bottom:1px solid rgba(201,168,76,0.05)">
                        <span class="symbol" style="color:${asp.color};font-size:1.2rem">${asp.symbol}</span>
                    </td>`;
                } else {
                    html += `<td class="aspect-cell" style="border-right:1px solid rgba(201,168,76,0.05);border-bottom:1px solid rgba(201,168,76,0.05)"></td>`;
                }
            } else if (i === j) {
                html += `<td style="background:rgba(201,168,76,0.03);border-right:1px solid rgba(201,168,76,0.05);border-bottom:1px solid rgba(201,168,76,0.05)"></td>`;
            } else {
                html += `<td></td>`;
            }
        }
        html += '</tr>';
    }

    html += '</table>';
    container.innerHTML = html;
}

export function renderAspectsList(chart) {
    const tbody = document.getElementById('aspectsBody');
    if (!tbody) return;

    const majorOnly = document.getElementById('majorOnlyToggle')?.checked;
    const applyingOnly = document.getElementById('applyingOnlyToggle')?.checked;

    let filtered = chart.aspects;
    if (majorOnly) filtered = filtered.filter(a => a.major);
    if (applyingOnly) filtered = filtered.filter(a => a.applying === true);

    let html = '';
    for (const asp of filtered) {
        html += `<tr>
      <th scope="row">${PLANET_GLYPHS[asp.planet1] || ''} ${asp.planet1}</th>
      <td style="color:${asp.color};font-size:1.2rem">${asp.symbol}</td>
      <td>${asp.aspectName}</td>
      <td style="color:${asp.color};font-size:1.2rem">${asp.symbol}</td>
      <td>${PLANET_GLYPHS[asp.planet2] || ''} ${asp.planet2}</td>
      <td>${asp.orb.toFixed(2)}°</td>
      <td style="color:${asp.applying === true ? 'var(--success)' : 'var(--text-muted)'}">${aspectPhaseLabel(asp)}</td>
    </tr>`;
    }
    tbody.innerHTML = html || '<tr><td colspan="7" style="text-align:center;color:var(--text-muted)">No aspects found</td></tr>';
}

export function aspectPhaseLabel(asp) {
    if (asp.applying === true) return 'Applying';
    if (asp.separating === true) return 'Separating';
    if (asp.futureOrb == null) return 'Phase unavailable';
    return 'Exact';
}

export function renderDignitiesTable(chart) {
    const tbody = document.getElementById('dignitiesBody');
    if (!tbody) return;

    let html = '';
    for (const [name, d] of Object.entries(chart.dignities)) {
        const glyph = PLANET_GLYPHS[name] || '';
        const scoreColor = d.score > 0 ? 'var(--success)' : d.score < 0 ? 'var(--danger)' : 'var(--text-dim)';
        const barW = Math.min(100, Math.abs(d.score) / 5 * 100);
        const barClass = d.score >= 0 ? 'dignity-positive' : 'dignity-negative';

        html += `<tr>
      <td style="font-size:1.2rem">${glyph}</td>
      <th scope="row">${name}</th>
      <td>${d.domicile ? 'yes' : ''}</td>
      <td>${d.exaltation ? 'yes' : ''}</td>
      <td>${d.triplicityRuler ? 'yes' : ''}</td>
      <td>${d.termRuler ? 'yes' : ''}</td>
      <td>${d.faceRuler ? 'yes' : ''}</td>
      <td style="color:${scoreColor}">
        ${d.score > 0 ? '+' : ''}${d.score}
        <div class="dignity-bar"><div class="dignity-bar-fill ${barClass}" style="width:${barW}%"></div></div>
      </td>
    </tr>`;
    }
    tbody.innerHTML = html;
}
