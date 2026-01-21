DEFINE TABLE repo SCHEMALESS;
DEFINE TABLE file SCHEMALESS;
DEFINE TABLE frag SCHEMALESS;
DEFINE TABLE symbol SCHEMALESS;

DEFINE ANALYZER code_analyzer
  TOKENIZERS class, blank, punct, camel
  FILTERS lowercase, ascii;

DEFINE INDEX frag_text_idx
  ON TABLE frag
  FIELDS retrieval_text
  SEARCH ANALYZER code_analyzer
  BM25;

DEFINE INDEX frag_vec_idx
  ON TABLE frag
  FIELDS embedding
  HNSW DIMENSION $EMBED_DIM DIST COSINE TYPE F32;

DEFINE INDEX file_repo_path ON TABLE file FIELDS repo_id, path;
DEFINE INDEX frag_repo_file ON TABLE frag FIELDS repo_id, file_id;
DEFINE INDEX frag_repo_kind ON TABLE frag FIELDS repo_id, kind;
DEFINE INDEX symbol_repo_symbol ON TABLE symbol FIELDS repo_id, symbol;
