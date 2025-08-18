CREATE TABLE "attachment_cache" (
    "attachment_id" INTEGER PRIMARY KEY
    , "path" TEXT NOT NULL UNIQUE
    , "hash" TEXT NOT NULL
    , "content" BLOB NOT NULL
);

CREATE TABLE "file_cache" (
    "file_id" INTEGER PRIMARY KEY
    , "path" TEXT NOT NULL UNIQUE
    , "hash" TEXT NOT NULL
);

CREATE TABLE "dep_cache" (
    "dep_id" INTEGER PRIMARY KEY
    , "path" TEXT NOT NULL
    , "hash" TEXT NOT NULL
    , "needs_path" TEXT NOT NULL
);
