import { DEFAULT_HORARY_SETTINGS } from '../astro/horarySettings.js';
import { getHouseSystems } from '../astro/houses.js';
import { escapeHtml } from './html.js';
import {
    renderAspectGrid,
    renderAspectsList,
    renderChartWheel,
    renderCuspsTable,
    renderDignitiesTable,
    renderPositionsTable,
} from './renderers.js';
import {
    renderHoraryAntiscia,
    renderHoraryAspects,
    renderHoraryFactors,
    renderHoraryPositions,
    renderHoraryReceptions,
    renderHoraryTimingPatterns,
} from './horaryRenderers.js';

const DEFAULT_RENDERERS = Object.freeze({
    renderAspectGrid,
    renderAspectsList,
    renderChartWheel,
    renderCuspsTable,
    renderDignitiesTable,
    renderHoraryAntiscia,
    renderHoraryAspects,
    renderHoraryFactors,
    renderHoraryPositions,
    renderHoraryReceptions,
    renderHoraryTimingPatterns,
    renderPositionsTable,
});

export function renderHoraryDashboard(chart, {
    documentRef = globalThis.document,
    houseSystems = getHouseSystems(),
    renderers = DEFAULT_RENDERERS,
} = {}) {
    const container = documentRef?.getElementById?.('horaryChart');
    if (!container) return false;

    const horaryHouseSystem = houseSystems.find(system => system.key === (chart.houseSystem || DEFAULT_HORARY_SETTINGS.houseSystem));
    const horaryHouseSystemName = horaryHouseSystem?.name || chart.houseSystem || DEFAULT_HORARY_SETTINGS.houseSystem;

    container.innerHTML = `
    <div class="panel panel-glow">
      <div class="chart-container" id="horaryWheelContainer"></div>
    </div>
    <div class="panel" style="margin-top:1rem">
      <h3 style="margin-bottom:0.75rem">Horary Positions (${escapeHtml(horaryHouseSystemName)})</h3>
      <div class="scroll-x">
        <table class="data-table">
          <caption class="sr-only">Horary positions</caption>
          <thead><tr><th scope="col" aria-label="Glyph"></th><th scope="col">Planet</th><th scope="col">Position</th><th scope="col">Sign</th><th scope="col">House</th><th scope="col">Status</th></tr></thead>
          <tbody>${renderers.renderHoraryPositions?.(chart) || ''}</tbody>
        </table>
      </div>
    </div>
    <div class="panel" style="margin-top:1rem">
      <h3 style="margin-bottom:0.75rem">Horary Factors</h3>
      <div class="scroll-x">
        <table class="data-table">
          <caption class="sr-only">Horary factors</caption>
          <tbody>${renderers.renderHoraryFactors?.(chart) || ''}</tbody>
        </table>
      </div>
    </div>
    <div class="panel" style="margin-top:1rem">
      <h3 style="margin-bottom:0.75rem">Receptions</h3>
      <div class="scroll-x">
        <table class="data-table">
          <caption class="sr-only">Receptions</caption>
          <thead><tr><th scope="col">Receiving Planet</th><th scope="col" aria-label="Reception relationship"></th><th scope="col">Received Planet</th><th scope="col">Dignity</th></tr></thead>
          <tbody>${renderers.renderHoraryReceptions?.(chart) || ''}</tbody>
        </table>
      </div>
    </div>
    <div class="panel" style="margin-top:1rem">
      <h3 style="margin-bottom:0.75rem">Antiscia</h3>
      <div class="scroll-x">
        <table class="data-table">
          <caption class="sr-only">Antiscia contacts</caption>
          <thead><tr><th scope="col">Planet</th><th scope="col">Contact</th><th scope="col">Planet</th><th scope="col">Orb</th></tr></thead>
          <tbody>${renderers.renderHoraryAntiscia?.(chart) || ''}</tbody>
        </table>
      </div>
    </div>
    <div class="panel" style="margin-top:1rem">
      <h3 style="margin-bottom:0.75rem">Timing Pattern Candidates</h3>
      <div class="scroll-x">
        <table class="data-table">
          <caption class="sr-only">Timing pattern candidates</caption>
          <thead><tr><th scope="col">Type</th><th scope="col">Candidate</th><th scope="col">Estimate</th></tr></thead>
          <tbody>${renderers.renderHoraryTimingPatterns?.(chart) || ''}</tbody>
        </table>
      </div>
    </div>
    <div class="panel" style="margin-top:1rem">
      <h3 style="margin-bottom:0.5rem">Applying Aspects</h3>
      <div class="scroll-x">
        <table class="data-table">
          <caption class="sr-only">Applying major aspects</caption>
          <thead><tr><th scope="col">Planet</th><th scope="col">Aspect</th><th scope="col">Planet</th><th scope="col">Orb</th></tr></thead>
          <tbody>${renderers.renderHoraryAspects?.(chart) || ''}</tbody>
        </table>
      </div>
    </div>
  `;

    renderers.renderChartWheel?.(chart, 'horaryWheelContainer');
    renderers.renderPositionsTable?.(chart);
    renderers.renderCuspsTable?.(chart);
    renderers.renderAspectGrid?.(chart);
    renderers.renderAspectsList?.(chart);
    renderers.renderDignitiesTable?.(chart);
    return true;
}
