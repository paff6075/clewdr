Vertex Using Documents

English | [简体中文](./vertex_zh.md)

**Step 1: Sign in to the GCP Console and Navigate to "APIs & Services"**

1. Open your web browser and go to the Google Cloud Console: `https://console.cloud.google.com/`
2. Sign in using your Google account.
3. Select the GCP project for which you want to create OAuth 2.0 credentials. If you don't have a project yet, create a new one first.

**Step 2: Go to the "Credentials" Page**

1. In the navigation menu on the left side of the console, find and click "APIs & Services".
2. In the expanded sub-menu, click "Credentials".

**Step 3: Create Credentials**

1. At the top of the "Credentials" page, click the "+ CREATE CREDENTIALS" button.
2. In the dropdown menu that appears, select "OAuth client ID".

**Step 4: Create OAuth Client ID - Select "Desktop app"**

After completing the OAuth consent screen configuration (if not already configured, you will be prompted to do so first), you may need to click "+ CREATE CREDENTIALS" -> "OAuth client ID" again.

* **Application type:** This is a crucial step. In the application type list, be sure to select "Desktop app".
* **Name:** Enter a descriptive name for this OAuth client ID so you can easily identify it in the console (e.g., "My Desktop App OAuth Client").

Click the "CREATE" button.

**Step 5: Obtain Your Client ID and Client Secret**

1. Upon successful creation, your "Your Client ID" and "Your Client Secret" will be displayed on the screen.
2. **Very Important:** Copy and securely save this information immediately. Your application will use these IDs and secrets to identify itself to Google's authentication service and request access to user data.
3. Click "OK" to close the dialog.

**Step 6: Obtain the Authorization Code via Browser**

Using the Client ID you obtained in Step 5, replace the `{YOUR_CLIENT_ID}` placeholder in the URL below and open this URL in your web browser to initiate the authorization flow:

`https://accounts.google.com/o/oauth2/auth?client_id={YOUR_CLIENT_ID}&redirect_uri=http://localhost&scope=https://www.googleapis.com/auth/cloud-platform&response_type=code&access_type=offline&prompt=consent`

* `client_id`: Your Client ID.
* `redirect_uri`: Must match the redirect URI used in your application (using `http://localhost` here).
* `scope`: The requested API permission scope (requesting access to Cloud Platform here).
* `response_type=code`: Indicates a request for an authorization code.
* `access_type=offline`: Requests a refresh token, allowing you to refresh the access token even when the user is offline.
* `prompt=consent`: Ensures the user sees the consent screen every time (optional, but recommended for first use).

After opening this URL in your browser, Google will prompt you to sign in (if not already signed in) and authorize your application to access the requested permissions. Upon consent, the browser will be redirected to the specified `redirect_uri` (`http://localhost`), including the authorization code in the URL's query parameters.

If everything goes well, the URL in your browser's address bar should look similar to this format:

`http://localhost/?code={YOUR_AUTHORIZATION_CODE}&scope=https://www.googleapis.com/auth/cloud-platform`

The `{YOUR_AUTHORIZATION_CODE}` in the URL is the authorization code you need. Copy and save this authorization code; it can only be used once and is short-lived.

**Step 7: Exchange the Authorization Code for Access and Refresh Tokens**

Open a terminal or command-line interface and use the `curl` command to send a POST request to Google's token endpoint to exchange the authorization code you obtained for an access token and a refresh token.

Replace `{YOUR_AUTHORIZATION_CODE}`, `{YOUR_CLIENT_ID}`, and `{YOUR_CLIENT_SECRET}` with your actual values:

```bash
curl -X POST \
  https://oauth2.googleapis.com/token \
  -d code={YOUR_AUTHORIZATION_CODE} \
  -d client_id={YOUR_CLIENT_ID} \
  -d client_secret={YOUR_CLIENT_SECRET} \
  -d redirect_uri=http://localhost \
  -d grant_type=authorization_code
```

* `code`: The authorization code you obtained in Step 6.
* `client_id`: Your Client ID.
* `client_secret`: Your Client Secret.
* `redirect_uri`: Must match the redirect URI used in Step 6.
* `grant_type=authorization_code`: Indicates you are using the authorization code flow.

If the request is successful, you will see a JSON response in your terminal similar to this:

```json
{
  "access_token": "ya29...",       // Token used to access APIs
  "expires_in": 3599,             // Access token expiration time (seconds)
  "scope": "https://www.googleapis.com/auth/cloud-platform",
  "token_type": "Bearer",
  "refresh_token": "1//0ad..."    // Token used to refresh the access token
}
```

Be sure to save `refresh_token`. The `refresh_token` usually has a longer lifespan (e.g., a week or more)

**Step 8: Fill in the Information to the ClewdR Frontend**

The information you need to enter includes:

* `client_id`: Your Client ID.
* `client_secret`: Your Client Secret.
* `refresh_token`: Your Refresh Token.
* `project_id`: Your Project ID.
* `model_id`: The model ID you want to use (optional, will use the model requested in the URL if left empty).
