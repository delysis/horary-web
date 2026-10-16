import { calculateAccidentalDignities } from './accidentalDignity.js';
import { calculateAntisciaContacts } from './antiscia.js';
import { calculateAspects } from './aspects.js';
import { getChartDignities, isDayChart } from './dignities.js';
import { DEFAULT_HORARY_SETTINGS } from './horarySettings.js';
import { calculateHouses } from './houses.js';
import { calculatePartOfFortune } from './lots.js';
import { calculatePlanetaryHour } from './planetaryHours.js';
import { getAllPositions } from './planets.js';
import { calculateReceptions } from './receptions.js';
import { calculateSolarConditions } from './solarCondition.js';
import { dateToJD, julianCenturies, lmst as calcLMST, meanObliquity, nutation } from './time.js';
import { calculateHoraryTimingPatterns } from './timingPatterns.js';
import { CLASSICAL_PLANETS, norm360 } from './utils.js';
import { calculateVoidOfCourseMoon } from './voidOfCourse.js';

export const MODERN_PLANETS = [
    'Sun',
    'Moon',
    'Mercury',
    'Venus',
    'Mars',
    'Jupiter',
    'Saturn',
    'Uranus',
    'Neptune',
    'Pluto',
    'NorthNode',
];

export const ASPECT_PHASE_EPSILON_DAYS = DEFAULT_HORARY_SETTINGS.aspectPhaseEpsilonDays;

export function planetListForSet(planetSet = 'modern') {
    return planetSet === 'classical' ? CLASSICAL_PLANETS : MODERN_PLANETS;
}

export function calculateChart({
    date,
    lat,
    lng,
    localDate = null,
    localTime = null,
    offsetHours = null,
    houseSystem = DEFAULT_HORARY_SETTINGS.houseSystem,
    planetSet = DEFAULT_HORARY_SETTINGS.planetSet,
    aspectConfig,
}) {
    const jd = dateToJD(date);
    const T = julianCenturies(jd);
    const obliquity = meanObliquity(T) + nutation(T).dEps;
    const siderealTime = calcLMST(jd, lng);
    const planetList = planetListForSet(planetSet);

    const positions = getAllPositions(jd, planetList);
    const futurePositions = getAllPositions(jd + ASPECT_PHASE_EPSILON_DAYS, planetList);
    const houses = calculateHouses(houseSystem, siderealTime, lat, obliquity);
    const aspects = calculateAspects(positions, { majorOnly: false, aspectConfig, futurePositions });
    const dayChart = isDayChart(positions.Sun.longitude, houses.asc);
    const dignities = getChartDignities(positions, dayChart);
    const receptions = calculateReceptions(positions, { isDayChart: dayChart });
    const voidOfCourseMoon = calculateVoidOfCourseMoon({ jd, positions });
    const antisciaContacts = calculateAntisciaContacts(positions);
    const solarConditions = calculateSolarConditions(positions, { planets: planetList });
    const planetaryHour = calculatePlanetaryHour({ date, lat, lng });
    const timingPatterns = calculateHoraryTimingPatterns({ aspects, positions, jd, planets: planetList });
    const partOfFortune = calculatePartOfFortune({
        ascendantLongitude: houses.asc,
        sunLongitude: positions.Sun.longitude,
        moonLongitude: positions.Moon.longitude,
        isDayChart: dayChart,
    });

    for (const pos of Object.values(positions)) {
        pos.house = getHouseNumber(pos.longitude, houses.cusps);
    }
    partOfFortune.house = getHouseNumber(partOfFortune.longitude, houses.cusps);
    const accidentalDignities = calculateAccidentalDignities(positions, { planets: planetList, solarConditions });

    return {
        jd,
        date,
        lat,
        lng,
        T,
        obliquity,
        siderealTime,
        positions,
        houses,
        aspects,
        dignities,
        accidentalDignities,
        receptions,
        voidOfCourseMoon,
        antisciaContacts,
        solarConditions,
        planetaryHour,
        timingPatterns,
        lots: {
            partOfFortune,
        },
        input: {
            localDate,
            localTime,
            offsetHours: Number.isFinite(offsetHours) ? offsetHours : null,
        },
        dayChart,
        aspectPhaseEpsilonDays: ASPECT_PHASE_EPSILON_DAYS,
        zodiac: DEFAULT_HORARY_SETTINGS.zodiac,
        houseSystem,
    };
}

export function getHouseNumber(longitude, cusps) {
    const lon = norm360(longitude);
    for (let i = 0; i < 12; i++) {
        const next = (i + 1) % 12;
        const start = norm360(cusps[i]);
        const end = norm360(cusps[next]);
        const dLon = norm360(lon - start);
        const dEnd = norm360(end - start);
        if (dEnd < 0.001) continue;
        if (dLon < dEnd) return i + 1;
    }
    return 1;
}
