Feature: Parallel per-image work and content-hash caches
  As a developer running pdfcn in a server, a batch or `pdfcn dev`
  I want independent image work spread over the cores and repeated work skipped
  So that a warm process renders faster without the PDF depending on either

  @covers(pdfcn-core/src/parallel.rs)
  Scenario: Spreading work over threads without changing its order
    Given a list of independent jobs and a thread budget
    When they are mapped in parallel
    Then the results come back in the input's order on any number of threads
    And PDFCN_THREADS sets the budget, with_threads overrides it on one thread only

  @covers(pdfcn-core/tests/threads.rs)
  Scenario: The same PDF with one thread or many
    Given a document with passed-through photos, cover crops and SVG marks
    When it is rendered with 1, 2, 4 and 7 threads, and again from warm caches
    Then every PDF is identical apart from its dates and file identifier

  @covers(pdfcn-core/src/cache.rs)
  Scenario: Keeping results by what went in, within a byte budget
    Given a cache with a budget in bytes
    When entries are added beyond it
    Then the least recently used ones are evicted and an oversized one is not kept
    And a key changes with the content and with its length, never with time

  @covers(pdfcn-core/src/vector_pdf.rs)
  Scenario: Converting a logo once per process
    Given the same SVG in several documents rendered by one process
    When the vector pass prepares them
    Then the SVG is converted the first time and reused afterwards

  @covers(pdfcn-core/src/assets.rs)
  Scenario: Hashing a photo only when it needs work
    Given a photo whose header says it needs no crop, rotation or shrink
    When the asset passes run
    Then its bytes are never hashed for the cache
    And a photo that does need work is prepared once per process

  @covers(api/generate-pdf-batch.rs)
  Scenario: A batch does not oversubscribe the cores
    Given a batch whose documents are already spread over threads
    When each document is laid out
    Then its per-image work runs on its own thread
