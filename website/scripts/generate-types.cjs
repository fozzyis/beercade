const { execFileSync } = require('child_process');
const path = require('path');

const repoRoot = path.resolve(__dirname, '../..');
const args = ['run', '-p', 'beercade-shared', '--bin', 'gen_ts_proto', '--', '--write'];

execFileSync('cargo', args, {
	cwd: repoRoot,
	stdio: 'inherit'
});
