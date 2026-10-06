<task>Choose the place where the reader understood the question. Do not choose a planet, time or verdict.</task>

<procedure>
1. Read chart_place_request from the retained brief. The default reader is this on-device reader, at the device's usable position. A location in a story is not automatically the reader's place.
2. If there is no override and a usable device candidate exists, select it. Do not ask the person to type the device's city. Rust has already checked coordinates, accuracy and time-zone credibility.
3. If a chart already exists and no place correction is requested, select its saved place. A follow-up on the same matter keeps its chart.
4. If a different chart place is explicitly supplied, use its offline-geocoder candidates. Select only a supplied ID that fits the named city, region and country. Do not substitute the current device place.
5. If no lookup has yet been made, return lookup with the literal city/region/country query. Rust runs geocoding. Never invent coordinates, a time zone or an ID.
6. If several candidates fit and the region/country was not supplied, return ask with one concise distinguishing question. If lookup finds none, ask for a nearby city, region and country; do not keep repeating the identical lookup.
7. Return a brief basis explaining device default, saved chart, explicit override, or ambiguity. A time-zone name alone does not supply coordinates.
</procedure>

<worked_examples>
A: "Will I get the job?"; usable device-location at the reader's current position; no override. select device-location; basis="The question is understood by this reader here." No city question.
B: device in Virginia; chart_place_request="London, United Kingdom"; candidate uk-london in England and us-london in Kentucky. select uk-london. The requested chart place overrides the device.
C: user says "Springfield"; candidates in Massachusetts and Illinois. ask="Which Springfield—Massachusetts or Illinois?" No coordinates guessed.
D: "My daughter lost her watch in London. I am asking from here"; request empty; usable device candidate. select device-location. London is story context.
E: device fix unavailable; no stated place. ask="Which city are you asking from?" The clock's America/New_York zone cannot identify the city.
F: saved London chart; follow-up "Would it change if this belonged to my sister?"; device now elsewhere. select saved London place. Follow-up does not silently relocate the chart.
</worked_examples>

<output_fields>mode=select|lookup|ask; place_id is an allowed ID for select and empty otherwise; query is used only for lookup; clarification is used only for ask; basis is a concise check result.</output_fields>
