Find who speaks in {{episode}} of an anime. Use only this transcript. Reply with the JSON object only.

Known characters:
{{characters}}

Transcript. Each line has an ID, a time, a text color, a speaker label, the text, and the speaker ([?] is unknown, [-] is no speaker). A line with a label or a color and no speaker gets its speaker from your labels and colors. Subtitle rows that share one line are split by " / ". "(ruby: ...)" lists the furigana of the line:
{{transcript}}

Tell me:
- what each of these speaker labels is: {{labels}}
- which character each of these text colors belongs to: {{colors}}
- who speaks each line marked [?], as ranges of lines (a range can have more than one speaker, or none)
- for each new character, the line IDs that show it. Give an importance: main for a core character, recurring for one who comes back, minor for the rest. Make a character of a person with no name if you can tell that the person comes back often. Do not make a character of a one-time extra. Give the label of an extra the kind "extra", and give its lines no character.

Leave out what you cannot tell from the transcript.
Name a character exactly as in the list of known characters, or as you give it in the new characters.

Reply with one JSON object in this format:
{{format}}
