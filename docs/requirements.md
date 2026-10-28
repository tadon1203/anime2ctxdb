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

- the entities: characters, places, and other things
- the scenes that ended before that point, and the scene in progress
- the facts, each with its real lines
- the lines of the current scene

The package holds nothing from after the chosen point. The AI that reads the package does the interpretation.

## What the user does

- The user gives source files and folders to the product. A folder includes its subfolders. The product proposes the episode and the language of each file, and shows where the episode came from. The user reviews the proposal and can change it or leave a file out before the save.
- The user approves or rejects suggestions from the product.
- The user copies prompts to an external AI tool and pastes the answers back.
- The user never enters story data by hand. The user can correct the episode and the language of an imported file.
- The user can remove an imported file from the series. The product asks first. The product keeps a subtitle that a prompt or an answer uses, because they point to its lines. The original file stays.

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
