# Security Policy

## Overview

This document outlines the security posture, policies, and best practices for the H-5N1P3R trading oracle system.

## Reporting Security Vulnerabilities

If you discover a security vulnerability, please report it by:

1. **DO NOT** create a public GitHub issue
2. Email the maintainers at [security contact - to be added]
3. Provide detailed information about the vulnerability including:
   - Description of the vulnerability
   - Steps to reproduce
   - Potential impact
   - Suggested fix (if any)

We will respond within 48 hours and work with you to address the issue promptly.

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Security Architecture

### Authentication & Authorization

#### Metrics Endpoint
- The Prometheus metrics endpoint (when enabled) requires authentication via bearer token
- Set `METRICS_AUTH_TOKEN` environment variable to enable authentication
- Use a cryptographically secure random token (minimum 32 characters)

#### API Access
- All external API calls (Solana RPC, Discord, Twitter, Telegram) use secure HTTPS connections
- API keys and tokens must be stored in environment variables, never in code
- Use private RPC endpoints for production deployments to avoid rate limiting

### Rate Limiting

Rate limiting is implemented at multiple levels:

1. **Governor-based rate limiting**: Built-in using the `governor` crate
2. **Per-endpoint limits**:
   - Metrics endpoint: 60 requests per minute per IP
   - Oracle queries: Configurable via `config.toml`
3. **Circuit breaker**: Automatic failure detection and recovery

### Input Validation

All external inputs are validated:

- **Token addresses**: Validated against Solana public key format
- **RPC responses**: Parsed with strict error handling
- **Configuration values**: Type-checked and range-validated
- **Database inputs**: Protected by SQLx parameterized queries (SQL injection safe)

### Secrets Management

#### Environment Variables
All sensitive data must be stored in environment variables:
- `RPC_URL`: Solana RPC endpoint
- `WALLET_PUBKEY`: Monitoring wallet address
- `METRICS_AUTH_TOKEN`: Metrics endpoint authentication
- API keys for external services (Discord, Twitter, Telegram)

#### Configuration Files
- `config.toml` should contain non-sensitive configuration only
- `config.toml` is gitignored by default (use `config.toml.template`)
- Never commit API keys, private keys, or tokens to version control

#### Best Practices
1. Use a `.env` file for local development (gitignored)
2. Use secure secret management systems in production (e.g., HashiCorp Vault, AWS Secrets Manager)
3. Rotate API keys and tokens regularly
4. Use minimum privilege principle for API keys
5. Never log sensitive data

### Database Security

- **SQLite database**: Uses local file storage (ensure proper file permissions)
- **SQL Injection**: Protected by SQLx compile-time checked queries
- **Data at rest**: Consider encrypting the database file in production
- **Backups**: Implement regular encrypted backups

### Dependency Security

#### Current Status

We regularly audit dependencies using `cargo audit`. Current known issues:

**Critical/High Severity:**
- ✅ **RUSTSEC-2024-0344** (curve25519-dalek v3.2.0): Timing vulnerability
  - **Status**: Indirect dependency via Solana SDK v2.0
  - **Impact**: Low - used only for signature verification, not sensitive operations
  - **Mitigation**: Monitoring Solana SDK updates; no known exploits in our usage pattern

**Medium Severity:**
- ⚠️ **RUSTSEC-2023-0071** (rsa v0.9.8): Marvin Attack potential
  - **Status**: Transitive dependency via sqlx[mysql] feature
  - **Impact**: None - we only use SQLite, MySQL feature is unused
  - **Mitigation**: Not applicable to our deployment; dependency cleanup planned

**Low/Info:**
- ℹ️ **RUSTSEC-2024-0375** (atty): Unmaintained
  - **Status**: Transitive via solana-logger
  - **Impact**: Low - logging functionality only
  - **Mitigation**: Monitoring for upstream fixes

- ℹ️ **RUSTSEC-2024-0388** (derivative): Unmaintained
  - **Status**: Transitive via ark-crypto dependencies in Solana
  - **Impact**: Low - used for macro derivations only
  - **Mitigation**: Monitoring for upstream fixes

- ℹ️ **RUSTSEC-2024-0436** (paste): Unmaintained
  - **Status**: Used by parquet crate and ark-crypto
  - **Impact**: Low - macro expansion only
  - **Mitigation**: Monitoring for upstream alternatives

- ℹ️ **RUSTSEC-2021-0145** (atty): Potential unaligned read
  - **Status**: Transitive via solana-logger
  - **Impact**: Low - platform-specific, unlikely to affect our deployment
  - **Mitigation**: Monitoring for upstream fixes

#### Dependency Update Policy

1. **Security updates**: Applied immediately upon availability
2. **Regular audits**: Run `cargo audit` on every build in CI/CD
3. **Major updates**: Reviewed and tested before merging
4. **Transitive dependencies**: Monitored via automated tools
5. **Dependency review**: All new dependencies undergo security review

#### Update Schedule
- Security patches: Immediate
- Minor updates: Weekly review
- Major updates: Monthly review
- Audit reports: Before each release

### Network Security

#### TLS/SSL
- All external HTTPS connections use TLS 1.2+ with certificate validation
- Solana RPC: Requires HTTPS endpoint for production
- No plaintext HTTP allowed for sensitive operations

#### Firewall & Network Segmentation
Production deployment recommendations:
- Restrict inbound connections to necessary ports only
- Use VPC/private networks for database access
- Implement IP whitelisting for metrics endpoint
- Use reverse proxy (nginx/Caddy) with rate limiting

### Operational Security

#### Logging
- Sensitive data is never logged (keys, tokens, private information)
- Use appropriate log levels (ERROR, WARN, INFO, DEBUG, TRACE)
- Log rotation and retention policies should be configured
- Consider centralized logging with encryption in transit

#### Monitoring
- Prometheus metrics expose system health (no sensitive data)
- Failed authentication attempts are logged
- Circuit breaker triggers are monitored
- Database connection errors are tracked

#### Deployment
- Run with minimal system privileges (non-root user)
- Use container security best practices if deploying with Docker
- Enable automatic security updates for OS packages
- Implement health checks and automatic restarts
- Use systemd hardening features on Linux

### Actor System Security

The system uses an actor-based architecture with built-in fault tolerance:
- **Supervision**: Automatic restart of failed actors
- **Isolation**: Actor state is isolated and not shared
- **Message validation**: All actor messages are type-checked
- **Circuit breakers**: Prevent cascade failures

## Security Testing

### Automated Testing
- Unit tests for input validation
- Integration tests for authentication
- Property-based testing for critical functions
- Fuzzing for parser code (planned)

### Security Scans
- `cargo audit` - dependency vulnerability scanning
- `cargo clippy` - Rust linter with security warnings
- CodeQL analysis - static analysis security testing
- Manual code review for security-critical changes

## Compliance & Best Practices

### Secure Coding Guidelines
1. Use Rust's type system for safety
2. Avoid `unsafe` code unless absolutely necessary (currently: 0 unsafe blocks)
3. Use parameterized queries for database access
4. Validate all external inputs
5. Handle all errors explicitly (no unwrap/expect in production code)
6. Use constant-time comparisons for secrets
7. Clear sensitive data from memory when no longer needed

### Data Protection
- Minimize data collection
- Encrypt sensitive data at rest and in transit
- Implement data retention policies
- Provide data export capabilities
- Respect privacy regulations (GDPR, CCPA, etc.)

## Incident Response

### Process
1. **Detection**: Automated monitoring and manual reports
2. **Analysis**: Assess impact and severity
3. **Containment**: Isolate affected systems
4. **Eradication**: Remove vulnerability or threat
5. **Recovery**: Restore normal operations
6. **Lessons Learned**: Post-incident review and documentation

### Emergency Contacts
[To be added - security team contacts]

## Changelog

### 2025-10-22
- Initial security policy created
- Documented current dependency vulnerabilities
- Added authentication for metrics endpoint with constant-time comparison
- Implemented IP-based rate limiting for HTTP endpoints
- Created comprehensive secrets management guidelines
- Added comprehensive security testing suite
- Integrated CodeQL security analysis

## Additional Resources

- [OWASP Top 10](https://owasp.org/www-project-top-ten/)
- [Rust Security Guidelines](https://anssi-fr.github.io/rust-guide/)
- [CWE/SANS Top 25](https://cwe.mitre.org/top25/)
- [Solana Security Best Practices](https://docs.solana.com/developing/programming-model/security)

## License

This security policy is part of the H-5N1P3R project and subject to the same license.
