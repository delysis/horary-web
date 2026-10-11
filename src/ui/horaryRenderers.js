import { signRuler } from '../astro/dignities.js';
import { PLANET_GLYPHS, signInfo } from '../astro/utils.js';
import { escapeHtml } from './html.js';
import { accidentalDignityLabel, dignityLabel } from './renderers.js';

export function renderHoraryPositions(chart) {
    const classical = ['Sun', 'Moon', 'Mercury', 'Venus', 'Mars', 'Jupiter', 'Saturn'];
    let html = '';
    for (const name of classical) {
        const pos = chart.positions[name];
        if (!pos) continue;
        const si = signInfo(pos.longitude);
        const glyph = PLANET_GLYPHS[name];
        const retroClass = pos.retrograde ? ' retrograde' : '';
        const dignity = chart.dignities[name];
        const status = dignity ? dignityLabel(dignity) : '';
        const accidentalStatus = accidentalDignityLabel(chart.accidentalDignities?.[name]);
        const dignityMarkup = [status, accidentalStatus].filter(Boolean).join('<br>');

        html += `<tr>
      <td style="font-size:1.2rem">${glyph}</td>
      <th scope="row" class="${retroClass}">${name}</th>
      <td>${si.degree}° ${si.glyph} ${String(si.minutes).padStart(2, '0')}'</td>
      <td><span class="sign-badge ${si.element}">${si.glyph} ${si.name}</span></td>
      <td>${pos.house || '-'}</td>
      <td>${dignityMarkup}</td>
    </tr>`;
    }
    return html;
}

export function renderHoraryFactors(chart) {
    const ascendantRuler = signRuler(signInfo(chart.houses.asc).index);
    const partOfFortune = chart.lots?.partOfFortune;
    const fortunePosition = partOfFortune
        ? `${formatLongitude(partOfFortune.longitude)} in house ${partOfFortune.house || '-'}`
        : 'Unavailable';
    const fortuneFormula = partOfFortune?.formula || '-';
    const voidOfCourse = chart.voidOfCourseMoon;
    const planetaryHour = chart.planetaryHour;

    return `
    <tr>
      <th scope="row">Chart sect</th>
      <td>${chart.dayChart ? 'Day' : 'Night'}</td>
    </tr>
    <tr>
      <th scope="row">Ascendant ruler</th>
      <td>${PLANET_GLYPHS[ascendantRuler] || ''} ${ascendantRuler || '-'}</td>
    </tr>
    <tr>
      <th scope="row">Part of Fortune</th>
      <td>${fortunePosition} <span style="color:var(--text-muted)">(${fortuneFormula})</span></td>
    </tr>
    <tr>
      <th scope="row">Void-of-course Moon</th>
      <td>${voidOfCourseLabel(voidOfCourse)}</td>
    </tr>
    <tr>
      <th scope="row">Solar condition</th>
      <td>${solarConditionsLabel(chart.solarConditions)}</td>
    </tr>
    <tr>
      <th scope="row">Planetary hour</th>
      <td>${planetaryHourLabel(planetaryHour)}</td>
    </tr>`;
}

export function renderHoraryReceptions(chart) {
    const receptions = chart.receptions || [];
    if (receptions.length === 0) {
        return '<tr><td colspan="4" style="text-align:center;color:var(--text-muted)">No receptions found</td></tr>';
    }

    return receptions.map(reception => `
    <tr>
      <th scope="row">${PLANET_GLYPHS[reception.hostPlanet] || ''} ${reception.hostPlanet}</th>
      <td>receives</td>
      <td>${PLANET_GLYPHS[reception.guestPlanet] || ''} ${reception.guestPlanet}</td>
      <td>${reception.dignity}${reception.mutual ? ' · mutual' : ''}</td>
    </tr>`).join('');
}

export function renderHoraryAntiscia(chart) {
    const contacts = chart.antisciaContacts || [];
    if (contacts.length === 0) {
        return '<tr><td colspan="4" style="text-align:center;color:var(--text-muted)">No antiscia contacts found</td></tr>';
    }

    return contacts.map(contact => `
    <tr>
      <th scope="row">${PLANET_GLYPHS[contact.planet1] || ''} ${contact.planet1}</th>
      <td>${contact.type === 'antiscion' ? 'antiscion' : 'contra-antiscion'}</td>
      <td>${PLANET_GLYPHS[contact.planet2] || ''} ${contact.planet2}</td>
      <td>${contact.orb.toFixed(2)}°</td>
    </tr>`).join('');
}

export function renderHoraryTimingPatterns(chart) {
    const patterns = chart.timingPatterns || [];
    if (patterns.length === 0) {
        return '<tr><td colspan="3" style="text-align:center;color:var(--text-muted)">No timing pattern candidates found</td></tr>';
    }

    return patterns.map(pattern => `
    <tr>
      <th scope="row">${timingPatternTypeLabel(pattern.type)}</th>
      <td>${timingPatternSummary(pattern)}</td>
      <td>${timingPatternEstimate(pattern)}</td>
    </tr>`).join('');
}

export function renderHoraryAspects(chart) {
    const applying = chart.aspects.filter(a => a.applying === true && a.major);
    if (applying.length === 0) return '<tr><td colspan="4" style="text-align:center;color:var(--text-muted)">No applying major aspects</td></tr>';

    let html = '';
    for (const asp of applying) {
        html += `<tr>
      <th scope="row">${PLANET_GLYPHS[asp.planet1]} ${asp.planet1}</th>
      <td style="color:${asp.color}">${asp.symbol} ${asp.aspectName}</td>
      <td>${PLANET_GLYPHS[asp.planet2]} ${asp.planet2}</td>
      <td>${asp.orb.toFixed(2)}°</td>
    </tr>`;
    }
    return html;
}

function formatLongitude(longitude) {
    const info = signInfo(longitude);
    return `${info.degree}° ${info.glyph} ${String(info.minutes).padStart(2, '0')}'`;
}

function voidOfCourseLabel(voidOfCourse) {
    if (!voidOfCourse?.available) return 'Unavailable';
    if (voidOfCourse.isVoid === true) {
        return `Yes <span style="color:var(--text-muted)">(checked ${voidOfCourse.checkedUntilHours}h to sign change)</span>`;
    }
    if (voidOfCourse.isVoid === false && voidOfCourse.nextApplyingAspect) {
        const aspect = voidOfCourse.nextApplyingAspect;
        return `No, Moon applies to ${PLANET_GLYPHS[aspect.planet] || ''} ${aspect.planet} by ${aspect.aspectName} in ${aspect.perfectsWithinHours}h`;
    }
    return voidOfCourse.reason || 'Unknown';
}

function planetaryHourLabel(planetaryHour) {
    if (!planetaryHour?.available) return planetaryHour?.reason || 'Unavailable';
    return `${PLANET_GLYPHS[planetaryHour.planetaryHourRuler] || ''} ${planetaryHour.planetaryHourRuler} hour ` +
        `(${planetaryHour.period} hour ${planetaryHour.hourNumber} of ${PLANET_GLYPHS[planetaryHour.planetaryDayRuler] || ''} ${planetaryHour.planetaryDayRuler} day)`;
}

function solarConditionsLabel(solarConditions = []) {
    if (!Array.isArray(solarConditions) || solarConditions.length === 0) {
        return 'No cazimi, combust, or under-beams planets';
    }

    return solarConditions.map(condition => {
        const planet = escapeHtml(condition.planet || '-');
        const label = solarConditionTypeLabel(condition.condition);
        const separation = Number.isFinite(condition.separationDegrees)
            ? `${condition.separationDegrees.toFixed(2)}° from Sun`
            : 'near Sun';
        return `${PLANET_GLYPHS[condition.planet] || ''} ${planet} ${label} <span style="color:var(--text-muted)">(${separation})</span>`;
    }).join('; ');
}

function solarConditionTypeLabel(condition) {
    if (condition === 'cazimi') return 'cazimi';
    if (condition === 'combust') return 'combust';
    if (condition === 'underBeams') return 'under beams';
    return escapeHtml(condition || 'near Sun');
}

function timingPatternTypeLabel(type) {
    if (type === 'translation') return 'Translation';
    if (type === 'collection') return 'Collection';
    if (type === 'prohibitionFrustration') return 'Prohibition/frustration';
    return type || 'Pattern';
}

function timingPatternSummary(pattern) {
    if (pattern.type === 'translation') {
        return `${planetLabel(pattern.mediator)} carries light from ${planetLabel(pattern.fromPlanet)} to ${planetLabel(pattern.toPlanet)}`;
    }
    if (pattern.type === 'collection') {
        return `${planetLabel(pattern.collector)} collects ${planetLabel(pattern.planet1)} and ${planetLabel(pattern.planet2)}`;
    }
    if (pattern.type === 'prohibitionFrustration') {
        return `${planetLabel(pattern.interveningPlanet)} perfects with ${planetLabel(pattern.sharedPlanet)} before ${planetLabel(pattern.planet1)}-${planetLabel(pattern.planet2)}`;
    }
    return 'Timing pattern candidate';
}

function timingPatternEstimate(pattern) {
    const hours = pattern.estimatedPerfectsWithinHours
        ?? pattern.estimatedSecondPerfectsWithinHours
        ?? pattern.interveningEstimatedPerfectsWithinHours;
    return Number.isFinite(hours) ? `${hours}h` : '-';
}

function planetLabel(planet) {
    return `${PLANET_GLYPHS[planet] || ''} ${planet || '-'}`.trim();
}
