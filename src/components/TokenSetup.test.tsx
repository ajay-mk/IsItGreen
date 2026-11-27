import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { AppError, ghStatus, glabStatus, tokens } from '../lib/transport';
import { TokenSetup } from './TokenSetup';

const TOKEN = 'github_pat_SECRET123';

/** Whether `a` comes before `b` in the document. */
function precedes(a: Element, b: Element) {
  return (a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0;
}

function setup(props: Partial<Parameters<typeof TokenSetup>[0]> = {}) {
  const onDone = vi.fn();
  const view = render(<TokenSetup onDone={onDone} {...props} />);
  const input = screen.getByLabelText('Access token') as HTMLInputElement;
  const submit = screen.getByRole('button', { name: 'Save token' });
  return { ...view, onDone, input, submit };
}

describe('TokenSetup', () => {
  it('stores the token through the backend and reports the login', async () => {
    vi.mocked(tokens.set).mockResolvedValue('octocat');
    const { input, submit, onDone, container } = setup();

    expect(input.type).toBe('password');
    expect(submit.hasAttribute('disabled')).toBe(true);
    await userEvent.type(input, TOKEN);
    await userEvent.click(submit);

    expect(tokens.set).toHaveBeenCalledExactlyOnceWith('github', TOKEN);
    await waitFor(() => expect(onDone).toHaveBeenCalledWith('octocat', 'github'));
    expect(input.value).toBe('');
    expect(container.innerHTML).not.toContain(TOKEN);
  });

  it('shows backend errors without echoing the token', async () => {
    vi.mocked(tokens.set).mockRejectedValue(new AppError('unauthorized', 'GitHub rejected this token.'));
    const { input, submit, onDone, container } = setup();

    await userEvent.type(input, TOKEN);
    await userEvent.click(submit);

    expect((await screen.findByRole('alert')).textContent).toBe('GitHub rejected this token.');
    expect(onDone).not.toHaveBeenCalled();
    // The value stays in the password field for a retry, but appears nowhere
    // in the markup, including the input's value attribute.
    expect(input.value).toBe(TOKEN);
    expect(container.innerHTML).not.toContain(TOKEN);
  });

  it('keeps the token out of the markup while typing', async () => {
    const { input, container } = setup();
    await userEvent.type(input, TOKEN);
    expect(input.getAttribute('value') ?? '').not.toContain('SECRET');
    expect(container.innerHTML).not.toContain('SECRET');
  });

  it('disables the button while verifying', async () => {
    let resolve: (login: string) => void = () => {};
    vi.mocked(tokens.set).mockReturnValue(new Promise((r) => (resolve = r)));
    const { input, submit } = setup();

    await userEvent.type(input, TOKEN);
    await userEvent.click(submit);
    expect(screen.getByRole('button', { name: 'Verifying…' }).hasAttribute('disabled')).toBe(true);

    resolve('octocat');
    await waitFor(() => expect(submit.textContent).toBe('Save token'));
  });

  it('keeps the button disabled for whitespace-only input', async () => {
    const { input, submit } = setup();
    await userEvent.type(input, '   ');
    expect(submit.hasAttribute('disabled')).toBe(true);
  });

  it('shows the reason and an optional cancel button', async () => {
    const onCancel = vi.fn();
    setup({ reason: 'Your token was rejected.', onCancel });
    expect(screen.getByText('Your token was rejected.')).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onCancel).toHaveBeenCalledOnce();
    expect(tokens.set).not.toHaveBeenCalled();
  });

  it('links to a prefilled token form and lists the permissions', () => {
    setup();
    const link = screen.getByRole('link', { name: 'Create a fine-grained token' });
    const url = new URL(link.getAttribute('href')!);
    expect(url.pathname).toBe('/settings/personal-access-tokens/new');
    expect(url.searchParams.get('pull_requests')).toBe('read');
    expect(link.closest('li')?.textContent).toContain('Metadata, Contents, Pull requests, Issues, Commit statuses and Actions');
  });

  describe('GitLab', () => {
    const GL_TOKEN = 'glpat-SECRET0123456789';

    async function chooseGitLab() {
      const view = setup();
      await userEvent.click(screen.getByRole('button', { name: 'GitLab' }));
      expect(screen.getByRole('heading', { name: 'Connect GitLab' })).toBeTruthy();
      return view;
    }

    it('stores a gitlab.com token by default', async () => {
      vi.mocked(tokens.set).mockResolvedValue('jane.doe');
      const { input, submit, onDone, container } = await chooseGitLab();
      expect(screen.getByRole('button', { name: 'GitLab' }).getAttribute('aria-pressed')).toBe('true');
      await userEvent.type(input, GL_TOKEN);
      await userEvent.click(submit);
      expect(tokens.set).toHaveBeenCalledExactlyOnceWith('gitlab', GL_TOKEN, null, 'gitlab.com');
      await waitFor(() => expect(onDone).toHaveBeenCalledWith('jane.doe', 'gitlab'));
      expect(container.innerHTML).not.toContain('SECRET');
    });

    it('sends a self-hosted instance in its normalized form', async () => {
      vi.mocked(tokens.set).mockResolvedValue('jane.doe');
      const { input, submit } = await chooseGitLab();
      const host = screen.getByRole('textbox', { name: 'GitLab instance' });
      await userEvent.clear(host);
      await userEvent.type(host, 'https://GitLab.Example.com:8443/');
      // The token page is shown as text: only connected instances can be opened.
      expect(screen.getByText(/gitlab\.example\.com:8443\/-\/user_settings\/personal_access_tokens/)).toBeTruthy();
      expect(screen.queryByRole('link', { name: 'Create a personal access token' })).toBeNull();
      await userEvent.type(input, GL_TOKEN);
      await userEvent.click(submit);
      expect(tokens.set).toHaveBeenCalledExactlyOnceWith('gitlab', GL_TOKEN, null, 'gitlab.example.com:8443');
    });

    it('refuses an instance address it cannot use', async () => {
      const { input, submit, onDone } = await chooseGitLab();
      const host = screen.getByRole('textbox', { name: 'GitLab instance' });
      await userEvent.clear(host);
      await userEvent.type(host, 'http://gitlab.example.com');
      await userEvent.type(input, GL_TOKEN);
      await userEvent.click(submit);
      expect(screen.getByRole('alert').textContent).toContain('Enter the address of your GitLab instance');
      expect(tokens.set).not.toHaveBeenCalled();
      expect(onDone).not.toHaveBeenCalled();
    });

    it('links to the prefilled token form on gitlab.com', async () => {
      await chooseGitLab();
      const link = screen.getByRole('link', { name: 'Create a personal access token' });
      const url = new URL(link.getAttribute('href')!);
      expect(url.host).toBe('gitlab.com');
      expect(url.searchParams.get('scopes')).toBe('read_api');
    });

    it('can open on GitLab', () => {
      render(<TokenSetup provider="gitlab" onDone={vi.fn()} />);
      expect(screen.getByRole('heading', { name: 'Connect GitLab' })).toBeTruthy();
    });

    it('keeps the connected instance when replacing its token', async () => {
      vi.mocked(tokens.set).mockResolvedValue('jane.doe');
      const { input, submit } = setup({ provider: 'gitlab', gitlabHost: 'gitlab.example.com' });
      expect((screen.getByRole('textbox', { name: 'GitLab instance' }) as HTMLInputElement).value).toBe(
        'gitlab.example.com',
      );
      await userEvent.type(input, GL_TOKEN);
      await userEvent.click(submit);
      expect(tokens.set).toHaveBeenCalledExactlyOnceWith('gitlab', GL_TOKEN, null, 'gitlab.example.com');
    });

    it('clears a typed token when switching service', async () => {
      const { input, submit } = setup();
      await userEvent.type(input, TOKEN);
      await userEvent.click(screen.getByRole('button', { name: 'GitLab' }));
      expect(input.value).toBe('');
      expect(submit.hasAttribute('disabled')).toBe(true);
    });
  });

  describe('GitHub CLI', () => {
    it('explains how to set up gh when it is not found, and checks again', async () => {
      vi.mocked(ghStatus).mockResolvedValue({ path: null, searched: ['/opt/homebrew/bin/gh', '/usr/local/bin/gh'] });
      setup();
      await userEvent.click(await screen.findByText('How to set up GitHub CLI'));
      expect(screen.queryByRole('button', { name: 'Use GitHub CLI login' })).toBeNull();
      expect(screen.getByText('/opt/homebrew/bin/gh')).toBeTruthy();
      expect(screen.getByText(/gh auth login/)).toBeTruthy();
      expect(screen.getByText(/ln -sf "\$\(which gh\)" \/usr\/local\/bin\/gh/)).toBeTruthy();

      // Installed in the meantime: checking again offers the login.
      vi.mocked(ghStatus).mockResolvedValue({ path: '/usr/local/bin/gh', searched: ['/usr/local/bin/gh'] });
      await userEvent.click(screen.getByRole('button', { name: 'Check again' }));
      expect(await screen.findByRole('button', { name: 'Use GitHub CLI login' })).toBeTruthy();
      expect(screen.queryByText('How to set up GitHub CLI')).toBeNull();
    });

    it('gives Windows install steps on Windows', async () => {
      vi.mocked(ghStatus).mockResolvedValue({ path: null, searched: ['C:\\Program Files\\GitHub CLI\\gh.exe'] });
      setup();
      await userEvent.click(await screen.findByText('How to set up GitHub CLI'));
      expect(screen.getByText('winget install --id GitHub.cli')).toBeTruthy();
      expect(screen.queryByText(/ln -sf/)).toBeNull();
    });

    it('links the GitHub CLI login and reports the account', async () => {
      vi.mocked(ghStatus).mockResolvedValue({ path: '/opt/homebrew/bin/gh', searched: ['/opt/homebrew/bin/gh'] });
      vi.mocked(tokens.linkGh).mockResolvedValue('octocat');
      const { onDone } = setup();
      await userEvent.click(await screen.findByRole('button', { name: 'Use GitHub CLI login' }));
      expect(tokens.linkGh).toHaveBeenCalledExactlyOnceWith();
      expect(tokens.set).not.toHaveBeenCalled();
      await waitFor(() => expect(onDone).toHaveBeenCalledWith('octocat', 'github'));
      expect(screen.getByText('/opt/homebrew/bin/gh')).toBeTruthy();
    });

    it('explains what to do when gh is signed out', async () => {
      vi.mocked(ghStatus).mockResolvedValue({ path: '/usr/bin/gh', searched: ['/usr/bin/gh'] });
      vi.mocked(tokens.linkGh).mockRejectedValue(
        new AppError('gh_not_logged_in', 'GitHub CLI is not signed in to this account. Run `gh auth login` in a terminal, then try again'),
      );
      const { onDone } = setup();
      await userEvent.click(await screen.findByRole('button', { name: 'Use GitHub CLI login' }));
      expect((await screen.findByRole('alert')).textContent).toContain('gh auth login');
      expect(onDone).not.toHaveBeenCalled();
    });

    it("offers each service's own CLI on its tab", async () => {
      // Found for one service and not the other, then the other way round,
      // so each tab must be asking about its own CLI.
      vi.mocked(ghStatus).mockResolvedValue({ path: '/usr/bin/gh', searched: ['/usr/bin/gh'] });
      setup();
      expect(await screen.findByRole('button', { name: 'Use GitHub CLI login' })).toBeTruthy();
      expect(screen.queryByText('GitHub CLI not found')).toBeNull();
      await userEvent.click(screen.getByRole('button', { name: 'GitLab' }));
      expect(await screen.findByText('GitLab CLI not found')).toBeTruthy();
      expect(screen.queryByRole('button', { name: 'Use GitHub CLI login' })).toBeNull();
      expect(screen.queryByRole('button', { name: 'Use GitLab CLI login' })).toBeNull();
    });
  });
  describe('CLI first', () => {
    const found = (tool: string) => ({ path: `/usr/bin/${tool}`, searched: [`/usr/bin/${tool}`] });

    it.each([
      ['found', true],
      ['not found', false],
    ])('puts the GitHub CLI before the token form when gh is %s', async (_, isFound) => {
      if (isFound) vi.mocked(ghStatus).mockResolvedValue(found('gh'));
      const { input } = setup();
      const cli = isFound
        ? await screen.findByRole('button', { name: 'Use GitHub CLI login' })
        : await screen.findByText('GitHub CLI not found');
      expect(precedes(cli, screen.getByText('or paste a personal access token'))).toBe(true);
      expect(precedes(screen.getByText('or paste a personal access token'), input)).toBe(true);
      if (!isFound) expect(precedes(screen.getByRole('button', { name: 'Check again' }), input)).toBe(true);
      // The token form stays usable below the CLI block.
      expect(screen.getByRole('button', { name: 'Save token' })).toBeTruthy();
    });

    it.each([
      ['found', true],
      ['not found', false],
    ])('puts the instance, then the GitLab CLI, then the token form when glab is %s', async (_, isFound) => {
      if (isFound) vi.mocked(glabStatus).mockResolvedValue(found('glab'));
      const { input } = setup({ provider: 'gitlab' });
      const cli = isFound
        ? await screen.findByRole('button', { name: 'Use GitLab CLI login' })
        : await screen.findByText('GitLab CLI not found');
      const hostField = screen.getByRole('textbox', { name: 'GitLab instance' });
      expect(precedes(hostField, cli)).toBe(true);
      expect(precedes(cli, input)).toBe(true);
      if (!isFound) expect(precedes(screen.getByRole('button', { name: 'Check again' }), input)).toBe(true);
      expect(screen.getByRole('button', { name: 'Save token' })).toBeTruthy();
    });
  });

  describe('GitLab CLI', () => {
    const GLAB = { path: '/opt/homebrew/bin/glab', searched: ['/opt/homebrew/bin/glab'] };

    it('links the GitLab CLI login for the instance typed in, normalized', async () => {
      vi.mocked(glabStatus).mockResolvedValue(GLAB);
      vi.mocked(tokens.linkGlab).mockResolvedValue('jane.doe');
      const { onDone } = setup({ provider: 'gitlab' });
      const host = screen.getByRole('textbox', { name: 'GitLab instance' });
      await userEvent.clear(host);
      await userEvent.type(host, 'https://GitLab.Example.com:8443/');
      await userEvent.click(await screen.findByRole('button', { name: 'Use GitLab CLI login' }));
      expect(tokens.linkGlab).toHaveBeenCalledExactlyOnceWith('gitlab.example.com:8443');
      expect(tokens.set).not.toHaveBeenCalled();
      await waitFor(() => expect(onDone).toHaveBeenCalledWith('jane.doe', 'gitlab'));
    });

    it('links gitlab.com by default and the connected instance when there is one', async () => {
      vi.mocked(glabStatus).mockResolvedValue(GLAB);
      vi.mocked(tokens.linkGlab).mockResolvedValue('jane.doe');
      const first = setup({ provider: 'gitlab' });
      await userEvent.click(await screen.findByRole('button', { name: 'Use GitLab CLI login' }));
      expect(tokens.linkGlab).toHaveBeenLastCalledWith('gitlab.com');
      first.unmount();

      setup({ provider: 'gitlab', gitlabHost: 'gitlab.example.com' });
      await userEvent.click(await screen.findByRole('button', { name: 'Use GitLab CLI login' }));
      expect(tokens.linkGlab).toHaveBeenLastCalledWith('gitlab.example.com');
    });

    it('refuses an instance address it cannot use', async () => {
      vi.mocked(glabStatus).mockResolvedValue(GLAB);
      const { onDone } = setup({ provider: 'gitlab' });
      const host = screen.getByRole('textbox', { name: 'GitLab instance' });
      await userEvent.clear(host);
      await userEvent.type(host, 'http://gitlab.example.com');
      await userEvent.click(await screen.findByRole('button', { name: 'Use GitLab CLI login' }));
      expect(screen.getByRole('alert').textContent).toContain('Enter the address of your GitLab instance');
      expect(tokens.linkGlab).not.toHaveBeenCalled();
      expect(onDone).not.toHaveBeenCalled();
    });

    it('explains how to set up glab for the instance, and checks again', async () => {
      vi.mocked(glabStatus).mockResolvedValue({ path: null, searched: ['/usr/bin/glab', '/snap/bin/glab'] });
      setup({ provider: 'gitlab', gitlabHost: 'gitlab.example.com' });
      await userEvent.click(await screen.findByText('How to set up GitLab CLI'));
      expect(screen.getByText('/snap/bin/glab')).toBeTruthy();
      expect(screen.getByText('glab auth login --hostname gitlab.example.com')).toBeTruthy();
      expect(screen.getByText('brew install glab')).toBeTruthy();

      vi.mocked(glabStatus).mockResolvedValue({ path: '/usr/bin/glab', searched: ['/usr/bin/glab'] });
      await userEvent.click(screen.getByRole('button', { name: 'Check again' }));
      expect(await screen.findByRole('button', { name: 'Use GitLab CLI login' })).toBeTruthy();
      expect(screen.queryByText('GitLab CLI not found')).toBeNull();
    });

    it('gives Windows install steps on Windows', async () => {
      vi.mocked(glabStatus).mockResolvedValue({ path: null, searched: ['C:\\Program Files\\glab\\glab.exe'] });
      setup({ provider: 'gitlab' });
      await userEvent.click(await screen.findByText('How to set up GitLab CLI'));
      expect(screen.getByText('winget install glab.glab')).toBeTruthy();
      expect(screen.queryByText(/ln -sf/)).toBeNull();
    });
  });
});
