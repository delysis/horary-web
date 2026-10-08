<task>Select NAMED native role-option IDs. Rust has bound each option to its person/object, counted turned houses and derived traditional rulers. Do not output house numbers, planets, custom labels or another role contract.</task>

<definitions>
A significator represents a person or thing in this question. Lord N rules the SIGN on house N's cusp, not the planet occupying it. A person's own role is distinct from their possessions. Husband Bob is seventh; his books as movable stock are second from seventh, absolute eighth. Bob does not become eighth because his books are eighth.

Editorial index from Frawley printed pp.15–29: 1 querent/body; 2 money/movables; 3 siblings/neighbours/routine communication; 4 father/home/land; 5 children/pleasures; 6 employees/services/illness/small animals; 7 partners/prospective partners/other parties; 8 death/partner's money; 9 higher learning/religion/long journeys; 10 mother/job/authority; 11 friends/hopes/employer's money; 12 confinement/large animals. Turning counts the owner's house as ONE. Rust performs the arithmetic.
</definitions>

<procedure>
1. Read the retained question, extracted people/subject, stage_user_replies and native_role_options. Do not assume a named person's relationship or who owns stock/objects.
2. If native_role_options.missing is nonempty, return request_input for a necessary missing relationship/ownership fact. The controller does not permit a data worksheet yet. Ask a distinguishing follow-up using prior replies instead of repeating an answered question.
3. Inspect each choice's ID, label, house/natural role and basis. Choose literal IDs; do not copy labels or numbers into invented fields. Required_groups lists required slots: select EXACTLY ONE ID from EACH group, not every ID in it. A group with two IDs means alternatives for ONE role. Compare both in comparison when requested, then choose one in selections during THIS call. A querent-only response cannot finish a question about Bob and his books. Conditional_roles lists additional obligations and their explicit exceptions; satisfy those too.
4. Keep the person's own option separate from their possessions: bob.self is Bob; subject.primary is the books in this case. The table binds those labels and houses. Do not substitute the stock option for Bob himself.
5. Justify relevance in each selection's reason. For an unmapped topic with ordinary-house alternatives, use the index and specific method. Do not choose a house because a planet occupies it.
6. Follow the method's actual Moon obligation. In relationship questions, select moon.contextual for the querent's emotions unless a selected main house ruler claims Moon. If Moon is already the querent's house ruler, retain its emotional meaning there; if it rules the person asked about, that person has first claim. In methods where Moon is optional, add it only for a stated purpose. Never create a competing natural Moon role. Do not invent gender, thieves, lovers or extra actors.
7. Native rejection leaves this same step unfinished. Correct the selection or ask for necessary user context. Do not ask for chart data; the table and house facts are supplied by the app.
</procedure>

<worked_examples>
A: unknown Bob in a book-sales question -> request_input field=subject_relationship; "Who is Bob to you?" A name does not prove he is a stranger.
B: stated husband Bob and his books -> select querent.self, bob.self, subject.primary. Native options bind Bob to 7 and Books to 8. Do not select subject.primary twice or omit bob.self while claiming all roles are assigned.
C: daughter's watch -> daughter.self=5, subject.primary=6. It is her possession, not the querent's ordinary second.
D: first house Cancer claims Moon -> leave out moon.contextual; retain the querent's emotional meaning on the primary Moon role, without a competing duplicate assignment.
E: required_groups=[["querent.self"],["subject.primary","subject.alternative_fourth"]]. Correct selections contains querent.self and ONE of the object IDs. Both object IDs belong in comparison, not both in selections. A rejection naming the group means fix that group's count; adding more roles will not resolve two competing alternatives.
</worked_examples>

<output_fields>selections=[{id,reason}], summary, unknowns; OR request_input. No roles/house/natural/owner_house/object_candidates fields. Computed facts are printed separately. Summary is two short sentences, not proof the assignments are correct.</output_fields>
