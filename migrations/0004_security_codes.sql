-- Security-feature labels move out of the database and into the application's
-- i18n catalog (they must be localizable per §12; a DB `label` column cannot be
-- translated per request). The set of valid codes is now a hardcoded list in
-- the domain (`SECURITY_FEATURE_CODES`), validated in Rust rather than by a FK.
--
-- `parking_security` keeps only (location_id, feature_code, state). We drop the
-- FK to `security_feature` and the catalog table itself.

ALTER TABLE parking_security
    DROP CONSTRAINT IF EXISTS parking_security_feature_code_fkey;

DROP TABLE IF EXISTS security_feature;
