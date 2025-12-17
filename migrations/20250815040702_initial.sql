CREATE TABLE "post" (
    -- rowid alias (`INTEGER PRIMARY KEY`) with strictly increasing automatic values (`AUTOINCREMENT`).
    -- this allows us to ensure that deleted post ids are never reused, which might get confusing, and
    -- it also allows us to ensure that potential cohost post ids (less than 10000000) are never used
    -- except when importing chosts, by inserting and deleting a dummy row with post id 9999999.
    -- <https://sqlite.org/lang_createtable.html#rowids_and_the_integer_primary_key>
    -- <https://sqlite.org/autoinc.html>
    "post_id" INTEGER PRIMARY KEY AUTOINCREMENT
    , "path" TEXT NOT NULL
    -- unique ignoring NULL values <https://sqlite.org/lang_createtable.html#unique_constraints>
    , "rendered_path" TEXT NULL UNIQUE
);

CREATE TABLE "import" (
    -- rowid alias (`INTEGER PRIMARY KEY`) with strictly increasing automatic values (`AUTOINCREMENT`).
    -- this allows us to ensure that deleted import ids are never reused, which might get confusing.
    -- <https://sqlite.org/lang_createtable.html#rowids_and_the_integer_primary_key>
    -- <https://sqlite.org/autoinc.html>
    "import_id" INTEGER PRIMARY KEY AUTOINCREMENT
    , "path" TEXT NOT NULL
);
