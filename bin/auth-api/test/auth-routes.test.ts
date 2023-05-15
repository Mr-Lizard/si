import t from 'tap';
import { expect } from 'chai';
import _ from 'lodash';

import { request } from './helpers/supertest-agents';
import { testSuiteAfter, testSuiteBefore } from './helpers/test-suite-hooks';
import { mockAuth0TokenExchange } from './helpers/auth0-mocks';
import { decodeAuthToken } from '../src/services/auth.service';

t.before(testSuiteBefore);
t.teardown(testSuiteAfter);

t.test('Auth routes', async () => {
  t.test('GET /auth/login - begin login flow', async (t) => {
    await t.test('redirects to auth0', async () => {
      await request.get('/auth/login')
        .expect(302)
        .expect((res) => {
        // example redirect url
        // https://systeminit.auth0.com/authorize?response_type=code&client_id=XXX&redirect_uri=http%3A%2F%2Flocalhost%3A9001%2Fauth%2Flogin-callback&state=ZZZ&scope=openid+profile+email'
          const redirectUrl = res.headers.location;
          expect(redirectUrl.startsWith(`https://${process.env.AUTH0_DOMAIN}/authorize?`)).to.eq(true);
        });
    });
  });

  t.test('GET /auth/login-callback - auth0 login callback', async (t) => {
    let validState: string;
    let validToken: string;
    const testEmail = `test-${+new Date()}@example.com`;

    t.test('(initiate login to get valid state)', async () => {
      await request.get('/auth/login')
        .expect(302)
        .expect((res) => {
          const redirectUrl = res.headers.location;
          // record the state value from our redirect url
          validState = redirectUrl.match(/state=([^&]+)/)[1];
        });
    });

    t.test(`works with a valid state`, async () => {

      mockAuth0TokenExchange({
        profileOverrides: { email: testEmail },
      });

      await request.get('/auth/login-callback')
        .query({
          code: 'mockedbutvalidcode',
          state: validState,
        })
        .expect(302)
        .expect(async (res) => {

          const setCookie = res.headers['set-cookie'][0];
          const [,authToken] = setCookie.match(/si-auth=([^;]+); path=\/; httponly/);
          validToken = authToken;

          const authData = await decodeAuthToken(authToken);
          expect(authData.userId).to.be.ok;

          expect(res.headers.location).to.eq(`${process.env.AUTH_PORTAL_URL}/login-success`);
        });
    });

    t.test('verify cookie works to make authenticated request', async () => {
      await request.get('/whoami')
        .set('cookie', `si-auth=${validToken};`)
        .expectOk()
        .expectBody({
          user: {
            email: testEmail,
          },
        });
    });

    t.test('verify bad cookie fails for authenticated request', async () => {
      await request.get('/whoami')
        .set('cookie', `si-auth=${validToken}X;`)
        .expectError('Unauthorized');
    });

    t.test(`fails if state is reused`, async () => {
      await request.get('/auth/login-callback')
        .query({
          code: 'mockedbutvalidcode',
          state: validState,
        })
        .expectError('Conflict');
    });

    _.each({
      'missing code': { code: undefined },
      'missing state': { state: undefined },
      // non-string values are treated as strings since they come in querystring
      // currently we do no other validation of if the values look like they are in the right format
    }, (queryOverride, description) => {
      t.test(`bad params - ${description}`, async () => {
        await request.get('/auth/login-callback')
          .query({
            code: 'somecode',
            state: 'somestate',
            ...queryOverride,
          })
          .expectError('BadRequest');
      });
    });

  });

  t.test('signup/login behaviour', async (t) => {

    // helper used to run a few tests about signup vs login behaviour and conflicting id/email
    async function runAuthTest(options: {
      mockOptions?: Parameters<typeof mockAuth0TokenExchange>[0],
      expectUserData?: any
    }) {

      // begin flow to get state
      const loginRes = await request.get('/auth/login');
      const validState = loginRes.headers.location.match(/state=([^&]+)/)[1];

      // trigger auth0 callback and mock token/profile requests
      mockAuth0TokenExchange(options.mockOptions);
      const loginCallbackRes = await request.get('/auth/login-callback')
        .query({
          code: 'mockedbutvalidcode',
          state: validState,
        })
        .expect(302);
      const setCookie = loginCallbackRes.headers['set-cookie'][0];
      const [,validToken] = setCookie.match(/si-auth=([^;]+); path=\/; httponly/);

      // use token to call whoami and get info about user
      const whoRes = await request.get('/whoami')
        .set('cookie', `si-auth=${validToken}`)
        .expectOk()
        .expectBody({ user: options?.expectUserData });
      return {
        userId: whoRes.body.user.id,
      };
    }

    const AUTH0_ID = 'google-oauth|12345';
    const EMAIL_1 = 'new-user@systeminit.dev';
    const EMAIL_2 = 'updated@systeminit.dev';

    let originalUserId: string;
    t.test('can sign up a new account', async () => {
      const { userId } = await runAuthTest({
        mockOptions: {
          profileOverrides: { sub: AUTH0_ID, email: EMAIL_1 },
        },
        expectUserData: { auth0Id: AUTH0_ID, email: EMAIL_1 },
      });
      originalUserId = userId;
    });

    t.test('logging in again with dupe email but different auth0 id will create new account', async () => {
      const { userId } = await runAuthTest({
        mockOptions: {
          profileOverrides: { sub: `${AUTH0_ID}9`, email: EMAIL_1 },
        },
        expectUserData: { email: EMAIL_1 },
      });
      expect(userId).not.to.eq(originalUserId);
    });

    t.test('logging in again with existing auth0 id will not create a new account, but will update other data', async () => {
      await runAuthTest({
        mockOptions: {
          profileOverrides: { sub: AUTH0_ID, email: EMAIL_2 },
        },
        expectUserData: { id: originalUserId, email: EMAIL_2 },
      });
    });
  });

  t.test('GET /auth/logout - begin logout flow', async (t) => {
    await t.test('redirects to auth0', async () => {
      await request.get('/auth/logout')
        .expect(302)
        .expect((res) => {
          // check redirects to auth0 logout, which will clear the cookie used for user <> auth0 requests
          const redirectUrl = res.headers.location;
          expect(redirectUrl.startsWith(`https://${process.env.AUTH0_DOMAIN}/v2/logout?`)).to.eq(true);

          // // check we cleared our cookie, which is used for user <> auth-api requests
          const setCookie = res.headers['set-cookie'][0];
          expect(setCookie).to.eq('si-auth=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT; httponly');
        });
    });
  });

  t.test('GET /auth/logout-callback - auth0 logout callback', async (t) => {
    await t.test('redirects to auth portal', async () => {
      await request.get('/auth/logout-callback')
        .expect(302)
        .expect((res) => {
          const redirectUrl = res.headers.location;
          expect(redirectUrl).to.eq(`${process.env.AUTH_PORTAL_URL}/login`);
        });
    });
  });

  t.test('signup', async () => {
    /*
      - duplicate emails are allowed and do not merge
      - first login will sign up, second will just log in
      - check default workspace is automatically created
    */

  });
});
