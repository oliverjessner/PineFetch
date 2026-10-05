import { spawn, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const tier = process.argv[2] ?? 'all';
if (process.argv.length > 3 || !['all', 'fast', 'integration'].includes(tier)) {
    console.error('Usage: node scripts/run-rust-tests.mjs [all|fast|integration]');
    process.exit(2);
}

// Existing names are retained. New tests crossing filesystem/process/TCP boundaries
// use integration_ so they join the same tier without growing this compatibility list.
const legacyIntegrationTests = new Set([
    'architecture_tests::process_start_errors_are_distinct_from_timeouts_without_parsing_messages',
    'cli::tests::sends_to_running_app_without_launching_and_propagates_errors',
    'cli::tests::launches_when_closed_then_sends_command_once',
    'cli::tests::launch_failure_is_reported_and_releases_startup_lock',
    'tests::canonicalizes_existing_local_paths',
    'tests::normalizes_only_usable_ffmpeg_locations',
    'tests::saves_caption_as_separate_utf8_text_file',
    'tests::selects_last_existing_output_path',
    'tests::selects_largest_format_part_when_final_output_is_missing',
    'tests::finds_related_format_parts_from_expected_output_path',
    'tests::timestamp_cut_keeps_an_existing_output_file',
    'tests::timed_out_child_is_stopped_promptly',
    'tests::inherited_output_pipe_cannot_block_after_parent_exits',
    'tests::cancellation_stops_a_child_and_its_process_group',
    'tests::app_exit_stops_registered_utility_child',
    'tests::removes_temporary_transcription_audio_when_dropped',
    'tests::computes_sha256_for_text_and_files',
    'tests::history_details_load_metadata_and_text_content_separately',
    'tests::history_details_report_only_available_content_sections',
]);

function terminateTree(child) {
    if (!child.pid) return;
    if (process.platform === 'win32') {
        spawnSync('taskkill', ['/PID', `${child.pid}`, '/T', '/F'], { timeout: 5000, stdio: 'ignore' });
        child.kill('SIGKILL');
        return;
    }
    // Production runners put utilities into separate groups. Include confirmed
    // descendants as well as the detached test harness group on outer timeout.
    const snapshot = spawnSync('ps', ['-axo', 'pid=,ppid='], { encoding: 'utf8', timeout: 2000 });
    const descendants = new Set([child.pid]);
    const processes = (snapshot.stdout ?? '')
        .trim()
        .split('\n')
        .map(line => line.trim().split(/\s+/).map(Number))
        .filter(fields => fields.length === 2 && fields.every(Number.isSafeInteger));
    let previousSize;
    do {
        previousSize = descendants.size;
        for (const [pid, parent] of processes) {
            if (descendants.has(parent)) descendants.add(pid);
        }
    } while (descendants.size !== previousSize);
    for (const pid of [...descendants].reverse()) {
        for (const target of [-pid, pid]) {
            try {
                process.kill(target, 'SIGKILL');
            } catch (error) {
                if (error.code !== 'ESRCH')
                    console.error(`[rust-tests] Cleanup failed for ${target}: ${error.message}`);
            }
        }
    }
}

function run(command, args, label, timeoutMs, display = false) {
    return new Promise((resolve, reject) => {
        const child = spawn(command, args, {
            cwd: root,
            detached: process.platform !== 'win32',
            stdio: ['ignore', 'pipe', 'pipe'],
        });
        let stdout = '';
        let stderr = '';
        const deadline = setTimeout(() => {
            terminateTree(child);
            child.stdout.destroy();
            child.stderr.destroy();
            child.unref();
            reject(new Error(`${label} exceeded ${timeoutMs / 1000}s; test processes were terminated`));
        }, timeoutMs);
        child.stdout.setEncoding('utf8').on('data', value => {
            stdout += value;
            if (display) process.stdout.write(value);
        });
        child.stderr.setEncoding('utf8').on('data', value => {
            stderr += value;
            if (display) process.stderr.write(value);
        });
        child.once('error', error => {
            clearTimeout(deadline);
            reject(new Error(`${label} could not start: ${error.message}`));
        });
        child.once('close', (code, signal) => {
            clearTimeout(deadline);
            if (code !== 0) {
                if (!display) {
                    process.stdout.write(stdout);
                    process.stderr.write(stderr);
                }
                reject(new Error(`${label} failed (${signal ?? `exit ${code}`})`));
            } else {
                resolve(stdout);
            }
        });
    });
}

function testTier(artifact, name) {
    return artifact.kind.includes('test') ||
        name.startsWith('integrity_tests::') ||
        name.split('::').at(-1).startsWith('integration_') ||
        legacyIntegrationTests.has(name)
        ? 'integration'
        : 'fast';
}

try {
    const messages = await run(
        'cargo',
        [
            'test',
            '--manifest-path',
            'src-tauri/Cargo.toml',
            '--locked',
            '--all-targets',
            '--no-run',
            '--message-format=json',
        ],
        'Cargo test compilation',
        15 * 60_000
    );
    const artifacts = new Map();
    for (const line of messages.trim().split('\n').filter(Boolean)) {
        const message = JSON.parse(line);
        if (message.reason !== 'compiler-artifact' || !message.profile.test || !message.executable) continue;
        const kind = message.target.kind;
        if (!kind.every(value => ['bin', 'lib', 'test'].includes(value))) {
            throw new Error(`Unclassified Cargo test artifact: ${message.target.name} (${kind.join(', ')})`);
        }
        artifacts.set(message.executable, { name: message.target.name, kind, executable: message.executable });
    }
    if (artifacts.size === 0) throw new Error('Cargo discovered no test artifacts');

    const suites = [];
    const counts = { fast: 0, integration: 0 };
    for (const artifact of artifacts.values()) {
        const listed = await run(
            artifact.executable,
            ['--list', '--format', 'terse'],
            `${artifact.name} test discovery`,
            30_000
        );
        const names = listed
            .trim()
            .split('\n')
            .filter(Boolean)
            .map(line => {
                const match = line.match(/^(.+): test$/);
                if (!match) throw new Error(`Unclassified test listing in ${artifact.name}: ${line}`);
                return match[1];
            });
        if (new Set(names).size !== names.length) throw new Error(`Duplicate test names in ${artifact.name}`);
        for (const name of names) counts[testTier(artifact, name)]++;
        suites.push({
            ...artifact,
            selected: names.filter(name => tier === 'all' || testTier(artifact, name) === tier),
        });
    }
    console.log(
        `[rust-tests] Discovered ${counts.fast + counts.integration} tests: ${counts.fast} fast, ${counts.integration} integration`
    );
    const selectedCount = suites.reduce((count, suite) => count + suite.selected.length, 0);
    if (selectedCount === 0) throw new Error(`No tests selected for ${tier}`);

    for (const suite of suites) {
        if (suite.selected.length === 0) continue;
        console.log(`[rust-tests] ${suite.name}: running ${suite.selected.length} ${tier} tests`);
        const output = await run(
            suite.executable,
            ['--exact', '--include-ignored', ...suite.selected],
            `${suite.name} ${tier} tests`,
            120_000,
            true
        );
        const summary = [...output.matchAll(/test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;/g)].at(-1);
        if (
            !summary ||
            Number(summary[1]) !== suite.selected.length ||
            Number(summary[2]) !== 0 ||
            Number(summary[3]) !== 0
        ) {
            throw new Error(`${suite.name} did not pass every selected test without ignoring tests`);
        }
    }
    console.log(`[rust-tests] ${tier}: ${selectedCount} tests passed; no selected tests ignored`);
} catch (error) {
    console.error(`[rust-tests] ${error.message}`);
    process.exitCode = 1;
}
