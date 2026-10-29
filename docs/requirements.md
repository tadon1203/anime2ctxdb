# Requirements

This document describes what the product does. It does not describe how.

## Purpose

The product turns an anime (video, audio, and subtitles) into a context database for AI. A person uses the database to write fan fiction with an AI. The AI must not misread the characters. It must not describe things the story has not shown yet. The context package is based on primary sources: the lines of the work.

## Input

- Subtitle files (ASS) are the first input. The product supports more kinds of sources later.
- Audio and video are later inputs. They are equal sources, not extras.
- The series can be finished or still airing.

## Output

A context package: YAML text that a person gives to an AI before it writes. The person chooses a point in the story (an episode and a time). The package holds what the story shows up to that point:

- the entities that matter: characters, places, and other things. An entity that matters has the importance `main` or `recurring`. An entity of the current scene is always in the package.
- for each character, its last 5 lines, so that the AI sees how the character speaks
- the scenes that ended before that point, and the scene in progress
- the facts of these entities, each with its real lines
- the lines of the current scene

The package holds nothing from after the chosen point. The AI that reads the package does the interpretation.

## Importance

Each entity has an importance: `main` (a core character), `recurring` (it comes back), or `minor` (the rest, and the default). The AI gives it when it introduces an entity, and can change it in a later answer. The latest answer up to the episode in question wins. The user can set the importance of an entity by hand in the Data tab or from the command line. A setting by hand wins over every answer, until the user removes it (`auto`).

A prompt lists the `main` and `recurring` entities. It lists a `minor` entity only when the entity appeared in the last episode. It lists only the facts of the entities that it lists.

## What the user does

- The user gives source files and folders to the product. A folder includes its subfolders. The product proposes the episode and the language of each file, and shows where the episode came from. The user reviews the proposal and can change it or leave a file out before the save.
- The answers become data at once. The user does not approve them. To fix an episode, the user answers it again, and the new answer replaces the old one.
- The user selects an episode and gets a prompt for it, of each kind (speakers and story). The user copies a prompt to an external AI tool and pastes the answer back. An episode has one answer of each kind. The user can look at what the answers gave. A person or an agent (Claude Code) can do this step from the command line.
- The user never enters story data by hand. The user can correct the episode and the language of an imported file, and can set the importance of an entity.
- The user can remove an imported file from the series. The product asks first. The product keeps a subtitle that an answer uses, because the answer points to its lines. The original file stays.

## Scope

In scope:

- read sources and extract speakers, characters, and scenes
- keep facts, each with its source; the reading AI does the interpretation
- find and filter data by story point
- build the context package
- check a finished draft for contradictions with the data (rule-based)

Out of scope:

- writing the fiction
- talking to an AI service
