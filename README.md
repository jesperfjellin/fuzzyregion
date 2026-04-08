# fuzzyregion

fuzzyregion is a PostgreSQL extension for storing and querying fuzzy regions on top of PostGIS.

The project is inspired by the 2025 paper [*Fuzzy Spatial Algebra (FUSA): Formal Specification of Fuzzy Spatial Data Types and Operations for Databases and GIS*](https://dl.acm.org/doi/10.1145/3722555), but it does not attempt to implement full FUSA from the start.

The MVP focuses on a single first-class SQL type, `fuzzyregion`, represented as a finite stack of nested alpha-cut polygons. This makes it possible to store vague areal phenomena in PostgreSQL, extract crisp views such as support, core, and alpha-cuts, and perform a small set of fuzzy set operations while relying on PostGIS as the underlying crisp geometry engine.