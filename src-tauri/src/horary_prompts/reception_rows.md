
<native_edge_contract>
This remains ONE worksheet for the whole supplied reception batch. Keep the existing direction, strength_and_quality and contextual_motive checks, summary and unknowns. Also return edge_interpretations: one row for EVERY supplied fact of kind reception that has a typed reception payload. Do not manufacture a row from legacy prose, a condition fact or a reverse edge that is not supplied. If none is supplied, return [] and keep reception unestablished or not_relevant.

For each row, do these small tasks in order:
1. Copy that fact's id into edge_id.
2. Match its reception.guestPlanet to native_reception.accepted_roles[*].planet. Copy ALL matching selected_id values into regarding_actor_ids. Copy IDs verbatim, not role labels, planet names, role indices or a new person name.
3. Match reception.hostPlanet in the same way into regarded_actor_ids. The guest regards the host; the host's regard requires its own separately supplied edge. When a planet has multiple accepted roles, retain every ID in the arrays. This mechanical alias list does not mean every role is equally operative in this question; do not blend people and objects into one actor.
4. Copy each positive dignity name from reception.qualities into positive_qualities, and each negative dignity name into negative_qualities. Preserve their exact native spellings, such as triplicityRuler and termRuler. These are arrays of strings, not string-encoded JSON. Retain minor and contrary qualities. Do not sum, cancel, replace or repeat them.
5. Give ONE short interpretation sentence about the guest's operative role regarding the host in this question. Domicile is strong positive regard; exaltation can idealize; triplicity is moderate; term is minor and face even more trivial (printed pp.71–74). Minor interest alone is not strong willingness. Mixed testimony remains mixed (p.75). Translate the liking/loving metaphor through the actual question: aversion to a person, disinterest in a deal and possession of money are not interchangeable psychological claims. If the operative meaning is not established, say so rather than invent one.
6. Give short limitations: this edge alone establishes neither reverse regard, ability, an occasion, occurrence nor timing (printed p.44). Do not infer a diagnosis, qualifications, an identity conflict or a future contact from reception alone.

Mechanical illustration only: if the supplied guest has selected_id "alma.self", and the host has two accepted aliases "querent.self" and "subject.primary", the two actor arrays are ["alma.self"] and ["querent.self","subject.primary"]. They are not ["Mars"], ["Alma"], [0] or a fabricated "person" ID. Use ONLY the actual current supplied IDs; these illustration IDs are not automatically available.

Each edge row has exactly seven fields: edge_id, regarding_actor_ids, regarded_actor_ids, positive_qualities, negative_qualities, interpretation, limitations. Keep interpretation within 240 characters and limitations within 180. Native acceptance checks the five identity/quality fields; it does not certify the truth of the two prose proposals.
</native_edge_contract>
