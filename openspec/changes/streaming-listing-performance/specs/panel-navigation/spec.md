## ADDED Requirements

### Requirement: Large directories stream progressively and stay responsive

The system SHALL show a directory's entries progressively while the directory is still being enumerated: the reader SHALL hand entries to the UI in batches bounded by an entry count and, while entries keep arriving, flushed at least about every 50 ms of enumeration time, so that the first entries and a growing `Reading… N` count appear before enumeration finishes, on local disks and slow network shares alike. Applying listing data SHALL NOT stall input handling or redrawing: each batch SHALL be merged into the sorted listing as a whole, in time proportional to the listing's size plus the cost of sorting the batch, never by inserting entries one at a time into the middle of the list; and the event loop SHALL apply at most about 8 ms or 64 messages of queued worker results before it processes pending input and redraws. Once the user has moved the cursor while a listing streams, arriving entries SHALL NOT move the cursor off the entry it is on, except where a same-directory re-read restores the cursor as "Same-directory re-read preserves cursor and scroll position" specifies. Name-based ordering and quick-filter matching SHALL use a lowercase name key computed once per entry when the entry is read, and the list of entries visible under the quick filter SHALL be recomputed only when the entries or the filter pattern change, so that, while no entries are selected, drawing a frame, hit-testing the mouse, and moving the cursor cost time proportional to the rows shown rather than to the directory's size.

#### Scenario: First entries appear before enumeration finishes

- **WHEN** a directory on a slow network share takes 20 seconds to enumerate and its entries arrive steadily throughout
- **THEN** entries and a growing `Reading… N` count are shown within about 50 ms of the first entries being read and keep growing while enumeration continues

#### Scenario: Input stays responsive while a 100,000-entry directory streams in

- **WHEN** a 100,000-entry directory is streaming in, many batches are already queued, and the user presses Down
- **THEN** the key is processed and the screen redrawn after at most about 8 ms or 64 queued worker messages, without waiting for the remaining batches to be applied

#### Scenario: Streaming 100,000 entries costs about one sort

- **WHEN** the release-build benchmark `bench_100k_ignored` streams 100,000 entries, in shuffled arrival order, into a panel in the reader's batch sizes
- **THEN** the total time spent applying the batches is at most three times the time of one bulk sort of the same 100,000 entries measured in the same run

#### Scenario: Cursor stays on its entry while entries stream in

- **WHEN** the user has moved the cursor to `report.txt` while the listing is still streaming, and a batch arrives containing entries that sort before `report.txt`
- **THEN** the cursor is still on `report.txt` after the batch is applied, and the entries remain in sorted order

#### Scenario: Frame cost does not grow with directory size

- **WHEN** the benchmark renders frames of a 100,000-entry panel and of a 1,000-entry panel with no entries selected, once with no quick filter and once with an active quick filter
- **THEN** a 100,000-entry frame takes no more than twice as long as a 1,000-entry frame under the same conditions

#### Scenario: Orders are unchanged by precomputed keys

- **WHEN** a listing is sorted by name, extension, time, or size, ascending or descending
- **THEN** the resulting order is identical to the case-insensitive orders and tie-breaks the panel produced before keys were precomputed, with `..` first
