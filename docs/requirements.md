# Requirements

This document describes what the product does. It does not describe how.

## Purpose

The product turns an anime (video, audio, and subtitles) into a context database for AI. A person uses the database to write fan fiction with an AI. The AI must not misread the characters. It must not describe things the story has not shown yet.

## Input

- Subtitle files (ASS) are the first input.
- Audio and video are later inputs. They are equal sources, not extras.
- The series can be finished or still airing.

## Output

A context package: text that a person gives to an AI before it writes. The person chooses a point in the story (an episode and a time), the characters, and the place. The package holds what is true at that point:

- the characters' traits, relationships, and speech style
- who knows what
- how characters call each other
- places and appearances

Every item has a source in the original work and a confidence. The package holds nothing from after the chosen point.

## What the user does

- The user gives source files to the product.
- The user approves or rejects proposals from the product.
- The user carries job files to an external AI tool and brings the responses back.
- The user never enters data by hand.

## Scope

In scope:

- read sources and extract speakers, characters, and scenes
- keep facts and interpretations apart, each with its source
- find and filter data by story point
- build the context package
- check a finished draft for contradictions with the data (rule-based)

Out of scope:

- writing the fiction
- talking to an AI service
