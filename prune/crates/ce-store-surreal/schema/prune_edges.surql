-- Edge tables (typed relations).
DEFINE TABLE imports SCHEMALESS TYPE RELATION IN file OUT file;
DEFINE TABLE contains SCHEMALESS TYPE RELATION IN file OUT frag;
DEFINE TABLE rel SCHEMALESS TYPE RELATION IN frag OUT frag;

-- Indexes for fast expansion by repo + direction.
DEFINE INDEX imports_repo_in ON TABLE imports FIELDS repo_id, in;
DEFINE INDEX imports_repo_out ON TABLE imports FIELDS repo_id, out;
DEFINE INDEX imports_repo_lang ON TABLE imports FIELDS repo_id, lang;

DEFINE INDEX contains_repo_in ON TABLE contains FIELDS repo_id, in;
DEFINE INDEX contains_repo_out ON TABLE contains FIELDS repo_id, out;
DEFINE INDEX contains_repo_kind ON TABLE contains FIELDS repo_id, kind;

DEFINE INDEX rel_repo_in ON TABLE rel FIELDS repo_id, in;
DEFINE INDEX rel_repo_out ON TABLE rel FIELDS repo_id, out;
DEFINE INDEX rel_repo_etype ON TABLE rel FIELDS repo_id, etype;
