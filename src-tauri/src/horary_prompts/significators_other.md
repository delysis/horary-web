<specific_method>
1. Use the house index for the specific question. Job/profession=10, own money=2, home/land=4, the other party/buyer/seller/opponent=7. Lord 1 remains the querent.
2. Turn ONLY when the actual subject belongs to someone else. Salary from a job is employer's second (11); partner's money is second from 7 (8). State the owner/relation before counting.
3. Add only roles relevant to the actual question. A job question is not an excuse to add a lover because Venus is present. An employment applicant can need job=10 and querent=1 without every other house.
4. If the subject's relationship or ownership is needed but missing, request_input pauses this step. Merely listing the missing relation in unknowns does not authorize a guessed house. If the method or calculation itself is unavailable, state that limit; do not ask the person to supply astrology.
5. Books held as movable stock are possessions: use their OWNER's second, rather than the third because books contain words. Do not call the third house a general house of local commerce. First establish whose books they are and who is selling them. A quantity question remains a quantity question; do not turn it into a binary marriage/sale event or invent a book count from an aspect's degrees.
</specific_method>

<worked_examples>
A: "Will I get the job?" Querent=1, job=10, Moon if not already claimed. Salary=11 is needed only if asked about salary.
B: "Will the buyer purchase my flat?" Querent=1, buyer=7, property=4 where relevant. Explain whose action is being tested.
C: "Will my partner receive their money?" Partner=7, their money=8. Do not automatically use the querent's 2nd.
D: "Will the judge favor me?" Querent=1, opponent=7 if relevant, judge=10. Essential rightness and accidental capacity differ; leave that assessment for its proper stage.
E: "How many books will Bob sell at the fair?" without Bob's relationship -> {"request_input":{"field":"subject_relationship","question":"Who is Bob to you?","reason":"His relationship determines whether to use an ordinary house or a turned house for him and his stock."}}. This is unfinished work, not a worksheet with an assumed seventh-house Bob.
F: "Bob is my husband; they are his books" -> husband=7; his movable stock=8 (second counted from 7), when that is the matter being judged. Books are not third-house stock merely because they are written. The ultimate sales/quantity judgment may need a relevant commercial role and a practical comparison; do not claim this example alone establishes a numerical prediction.
</worked_examples>

<output_fields>selections=[{id,reason}], summary, unknowns; OR request_input. Select only the supplied native option IDs. The native table computes ownership, turned houses and rulers. For the querents own missing object, include comparison observations for both listed candidates and select one object option. Do not output roles, house numbers, owner_house or object_candidates.</output_fields>
