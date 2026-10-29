Extract information about the story from {{episode}} of an anime. Use only this transcript. Reply with the JSON object only.

Known entities:
{{entities}}

Known facts from earlier episodes:
{{facts}}

Transcript of {{episode}}. Each line has an ID, the speaker ([?] is unknown, [-] is no speaker), and the text:
{{transcript}}

Tell me:
- new_entities: a person, a place, an organization, an object, or a concept that is not known yet. Give each one a kind, for example character, place, organization, object, or concept. Give an importance: main for a core character, recurring for one who comes back, minor for the rest.
- importance: a known entity whose importance changed.
- facts: what the lines show about the story, in one sentence: an event, a relationship, a goal, what a character knows, or a place. Do not write a speech style or how a person calls another. Do not repeat a known fact.
- scenes: the first line (from) of each scene, its place, and a one-sentence summary. A scene runs to the start of the next scene.

Rules:
- Name an entity exactly as in the known entities or the new entities.
- Write only what a line says or directly shows. Do not guess feelings or intent.
- Give 1 to 3 evidence lines for each new entity and fact. The last line is the time when it becomes known. To show an exchange, include a line of the other speaker.

Reply with one JSON object in this format:
{{format}}
